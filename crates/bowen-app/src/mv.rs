use crate::{
    desktop_service::{Command, Event},
    playback::PlaybackCoordinator,
};
pub use bowen_api::catalog::MvQuality;
use bowen_api::{catalog::mv_info_at_revision, transport::Transport};
use bowen_playback::{
    mpv::{LoadSource, MpvOptions, MpvPlayer, PlaybackState},
    video::{VideoPlayer, VideoReader, VideoSize},
};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc as std_mpsc},
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle};

type AudioPlayer = PlaybackCoordinator<MpvPlayer>;
const NO_MV: &str = "这首歌曲暂无 MV";

#[derive(Clone, Default)]
pub struct MvView {
    pub title: String,
    pub quality: Option<MvQuality>,
    pub requested: Option<MvQuality>,
    pub retry_quality: Option<MvQuality>,
    pub preview_seconds: Option<u32>,
    pub source: Option<VideoReader>,
    pub error: Option<&'static str>,
}

pub enum MvAction {
    Quality(MvQuality),
    Playing(Option<bool>),
    Seek(f64),
    Volume(f64),
}

pub struct MvCandidate {
    player: VideoPlayer,
    quality: MvQuality,
    preview_seconds: Option<u32>,
}

impl MvCandidate {
    fn clamp_seek(&self, position: f64) -> f64 {
        let limit = self.preview_seconds.map(f64::from).unwrap_or(f64::INFINITY);
        position.clamp(0., limit)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AudioResume {
    revision: u64,
    id: i64,
    loaded_count: u64,
}

impl AudioResume {
    fn matches(self, player: &AudioPlayer) -> bool {
        player.queue_account_revision() == self.revision
            && player
                .queue()
                .current()
                .is_some_and(|track| track.id == self.id)
            && player.snapshot().loaded_count == self.loaded_count
    }
}

struct Session {
    id: i64,
    revision: u64,
    request: u64,
    view: MvView,
    pending: Option<JoinHandle<()>>,
    current: Option<MvCandidate>,
    volume: f64,
}

pub(crate) struct MvController {
    transport: Arc<Transport>,
    commands: mpsc::UnboundedSender<Command>,
    events: std_mpsc::Sender<Event>,
    library: PathBuf,
    audio_output: Option<String>,
    generation: u64,
    session: Option<Session>,
    resume: Option<AudioResume>,
}

impl MvController {
    pub(crate) fn new(
        transport: Arc<Transport>,
        commands: mpsc::UnboundedSender<Command>,
        events: std_mpsc::Sender<Event>,
        library: PathBuf,
        audio_output: Option<String>,
    ) -> Self {
        Self {
            transport,
            commands,
            events,
            library,
            audio_output,
            generation: 0,
            session: None,
            resume: None,
        }
    }

    pub(crate) fn open(
        &mut self,
        revision: u64,
        id: i64,
        volume: f64,
        player: Option<&AudioPlayer>,
    ) {
        let Some(track) = player
            .filter(|player| player.queue_account_revision() == revision)
            .and_then(|player| player.queue().current())
            .filter(|track| {
                track.id == id && self.transport.session().snapshot().revision == revision
            })
        else {
            let _ = self
                .events
                .send(Event::Status("歌曲或账号已变化，请重试打开 MV".into()));
            return;
        };
        self.close(true);
        self.session = Some(Session {
            id,
            revision,
            request: 0,
            pending: None,
            current: None,
            volume,
            view: MvView {
                title: track.title.clone(),
                ..Default::default()
            },
        });
        self.load(MvQuality::High);
    }

    fn load(&mut self, quality: MvQuality) {
        let Some(session) = &mut self.session else {
            return;
        };
        if let Some(task) = session.pending.take() {
            task.abort();
        }
        self.generation = self.generation.wrapping_add(1);
        session.request = self.generation;
        session.view.requested = Some(quality);
        session.view.retry_quality = None;
        session.view.error = None;
        let _ = self.events.send(Event::Mv(Some(session.view.clone())));
        let request = session.request;
        let id = session.id;
        let revision = session.revision;
        let transport = Arc::clone(&self.transport);
        let library = self.library.clone();
        let audio_output = self.audio_output.clone();
        let commands = self.commands.clone();
        let position = session
            .current
            .as_ref()
            .map_or(0., |mv| mv.player.snapshot().position_seconds);
        session.pending = Some(tokio::spawn(async move {
            let result = prepare(
                transport,
                id,
                revision,
                quality,
                library,
                audio_output,
                position,
            )
            .await;
            let _ = commands.send(Command::MvLoaded(request, result));
        }));
    }

    pub(crate) fn loaded(
        &mut self,
        request: u64,
        result: Result<MvCandidate, &'static str>,
        audio: Option<&mut AudioPlayer>,
    ) {
        let Some(session) = self.session.as_mut().filter(|session| {
            session.request == request
                && session.revision == self.transport.session().snapshot().revision
        }) else {
            return;
        };
        session.pending = None;
        let requested = session.view.requested.take();
        let result = match result {
            Err(NO_MV) if session.current.is_some() => Err("该画质暂不可用"),
            result => result,
        };
        let installed = result.and_then(|mv| {
            let old = session.current.as_ref().map(|old| old.player.snapshot());
            if let Some(snapshot) = &old {
                let position = mv.clamp_seek(snapshot.position_seconds);
                mv.player
                    .seek(position)
                    .map_err(|_| "无法恢复 MV 播放进度")?;
            }
            mv.player
                .volume(session.volume)
                .map_err(|_| "无法设置 MV 音量")?;
            if let Some(old) = &session.current
                && old.player.snapshot().failure.is_none()
            {
                old.player.pause(true).map_err(|_| "无法切换 MV 画质")?;
            } else if session.current.is_none()
                && let Some(audio) = audio
                && audio.snapshot().state == PlaybackState::Playing
            {
                audio.pause(true).map_err(|_| "无法暂停当前歌曲")?;
                self.resume = Some(AudioResume {
                    revision: session.revision,
                    id: session.id,
                    loaded_count: audio.snapshot().loaded_count,
                });
            }
            if old.is_none_or(|snapshot| {
                snapshot.state == PlaybackState::Playing || snapshot.failure.is_some()
            }) && mv.player.pause(false).is_err()
            {
                if let Some(old) = &session.current {
                    let _ = old.player.pause(false);
                }
                return Err("无法开始播放 MV");
            }
            session.view.quality = Some(mv.quality);
            session.view.preview_seconds = mv.preview_seconds;
            session.view.source = Some(mv.player.reader());
            session.current = Some(mv);
            Ok(())
        });
        session.view.error = installed.err();
        if session.view.error.is_some() {
            session.view.retry_quality = requested;
        }
        let _ = self.events.send(Event::Mv(Some(session.view.clone())));
        if session.current.is_none()
            && let Some(resume) = self.resume
        {
            let _ = self.commands.send(Command::MvResumeAudio(resume));
        }
    }

    pub(crate) fn control(&mut self, action: MvAction) {
        if let MvAction::Quality(quality) = action {
            self.load(quality);
            return;
        }
        let Some(session) = &mut self.session else {
            return;
        };
        if let MvAction::Volume(volume) = action
            && volume.is_finite()
        {
            session.volume = volume.clamp(0., 100.);
        }
        let Some(mv) = &session.current else { return };
        let result = match action {
            MvAction::Playing(requested) => {
                let playing = mv.player.snapshot().state == PlaybackState::Playing;
                mv.player.pause(!requested.unwrap_or(!playing))
            }
            MvAction::Seek(position) if position.is_finite() => {
                mv.player.seek(mv.clamp_seek(position))
            }
            MvAction::Volume(_) => mv.player.volume(session.volume),
            _ => return,
        };
        if result.is_err() {
            session.view.error = Some("MV 播放操作失败，请重试");
            let _ = self.events.send(Event::Mv(Some(session.view.clone())));
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn reconcile(&mut self, revision: u64, player: Option<&AudioPlayer>) {
        if self.session.as_ref().is_some_and(|session| {
            session.revision != revision
                || player
                    .and_then(|player| player.queue().current())
                    .is_none_or(|track| track.id != session.id)
        }) {
            self.close(false);
        }
    }

    pub(crate) fn close(&mut self, resume_audio: bool) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        if let Some(task) = session.pending.take() {
            task.abort();
        }
        let resume = if resume_audio {
            self.resume
        } else {
            self.resume.take();
            None
        };
        let current = session.current.take();
        let commands = self.commands.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(mv) = current {
                mv.player.shutdown();
            }
            if let Some(resume) = resume {
                let _ = commands.send(Command::MvResumeAudio(resume));
            }
        });
        let _ = self.events.send(Event::Mv(None));
    }

    pub(crate) fn resume_audio(&mut self, resume: AudioResume, audio: Option<&mut AudioPlayer>) {
        if self.resume == Some(resume)
            && self
                .session
                .as_ref()
                .is_none_or(|session| session.current.is_none())
        {
            self.resume = None;
            if let Some(audio) = audio.filter(|audio| {
                resume.matches(audio) && audio.snapshot().state == PlaybackState::Paused
            }) {
                let _ = audio.pause(false);
            }
        }
    }
}

async fn prepare(
    transport: Arc<Transport>,
    id: i64,
    revision: u64,
    quality: MvQuality,
    library: PathBuf,
    audio_output: Option<String>,
    position: f64,
) -> Result<MvCandidate, &'static str> {
    let info = mv_info_at_revision(&transport, id, quality, revision)
        .await
        .map_err(|_| "无法获取 MV，请重试")?
        .ok_or(NO_MV)?;
    let preview_seconds = info.preview_limit_seconds;
    let end = preview_seconds.map(f64::from);
    let start = position.min(end.map_or(f64::INFINITY, |end| (end - 0.1).max(0.)));
    let player = tokio::task::spawn_blocking(move || {
        VideoPlayer::open(
            library,
            LoadSource {
                address: info.video_url,
                start_seconds: (start > 0.).then_some(start),
                end_seconds: end,
                initially_paused: true,
            },
            MpvOptions { audio_output },
            VideoSize::new(960, 540).expect("fixed MV surface size"),
        )
    })
    .await
    .map_err(|_| "无法启动 MV 播放器")?
    .map_err(|error| match error {
        bowen_playback::video::VideoError::RendererUnavailable
        | bowen_playback::video::VideoError::Mpv(bowen_playback::mpv::MpvError::Library) => {
            "无法加载支持视频的 libmpv，请检查 MV 播放组件"
        }
        _ => "无法打开 MV，请检查视频播放组件或重试",
    })?;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let snapshot = player.snapshot();
            if snapshot.failure.is_some() {
                return Err("MV 未能解码或加载，请检查视频播放组件或重试");
            }
            if snapshot.frame.is_some() && snapshot.state == PlaybackState::Paused {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .map_err(|_| "MV 加载超时，请重试")??;
    Ok(MvCandidate {
        player,
        quality,
        preview_seconds,
    })
}
