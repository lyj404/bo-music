use super::MusicApp;
use bo_music_app::desktop_service::{Command, PlayerView};
use bo_music_playback::mpv::PlaybackState;
use gpui::{Context, Image, Task, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{Arc, Mutex};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Media::{
        MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
        SystemMediaTransportControls as Controls, SystemMediaTransportControlsButton as Button,
        SystemMediaTransportControlsButtonPressedEventArgs as ButtonArgs,
        SystemMediaTransportControlsTimelineProperties as Timeline,
    },
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference},
    Win32::{
        Foundation::{E_FAIL, E_HANDLE, HWND},
        System::WinRT::{
            ISystemMediaTransportControlsInterop, RO_INIT_SINGLETHREADED, RoInitialize,
            RoUninitialize,
        },
    },
    core::{Error, HSTRING, Result, factory},
};

type TrackStamp = (u64, i64);

pub(super) struct WindowsMedia {
    controls: Controls,
    timeline: Timeline,
    button_token: Option<i64>,
    seek_token: Option<i64>,
    seek_track: Arc<Mutex<Option<TrackStamp>>>,
    metadata: Option<Metadata>,
    state: Option<(MediaPlaybackStatus, bool, bool)>,
    last_timeline: Option<(u64, u64)>,
    artwork_id: Option<u64>,
    artwork_task: Option<Task<()>>,
    _apartment: WinRtApartment,
}

#[derive(PartialEq, Eq)]
struct Metadata {
    stamp: TrackStamp,
    title: String,
    artist: String,
    album: Option<String>,
    cover_url: Option<String>,
}

struct WinRtApartment;

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        // Matches the successful initialization of this GPUI UI thread.
        unsafe { RoUninitialize() };
    }
}

impl WindowsMedia {
    fn new(window: &Window, send: impl Fn(Command) + Clone + Send + 'static) -> Result<Self> {
        let handle =
            HasWindowHandle::window_handle(window).map_err(|_| Error::from_hresult(E_HANDLE))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(Error::from_hresult(E_HANDLE));
        };
        // GPUI initializes OLE on this thread. Initialize WinRT in the same STA model.
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
        let apartment = WinRtApartment;
        let interop = factory::<Controls, ISystemMediaTransportControlsInterop>()?;
        // The borrowed HWND is valid for this window and belongs to this process.
        let controls: Controls = unsafe { interop.GetForWindow(HWND(handle.hwnd.get() as _))? };
        let mut media = Self {
            controls,
            timeline: Timeline::new()?,
            button_token: None,
            seek_token: None,
            seek_track: Arc::default(),
            metadata: None,
            state: None,
            last_timeline: None,
            artwork_id: None,
            artwork_task: None,
            _apartment: apartment,
        };
        let send_button = send.clone();
        media.button_token = Some(media.controls.ButtonPressed(&TypedEventHandler::<
            Controls,
            ButtonArgs,
        >::new(
            move |_, args| {
                if let Some(args) = args.as_ref() {
                    let command = match args.Button()? {
                        Button::Play => Command::SetPlaying(true),
                        Button::Pause => Command::SetPlaying(false),
                        Button::Stop => Command::Stop,
                        Button::Next => Command::Next,
                        Button::Previous => Command::Previous,
                        _ => return Ok(()),
                    };
                    send_button(command);
                }
                Ok(())
            },
        ))?);
        let seek_track = Arc::clone(&media.seek_track);
        media.seek_token = Some(media.controls.PlaybackPositionChangeRequested(
            &TypedEventHandler::<Controls, PlaybackPositionChangeRequestedEventArgs>::new(
                move |_, args| {
                    let stamp = seek_track.lock().ok().and_then(|stamp| *stamp);
                    if let (Some((account, track)), Some(args)) = (stamp, args.as_ref()) {
                        let ticks = args.RequestedPlaybackPosition()?.Duration;
                        if ticks >= 0 {
                            send(Command::SeekTo(account, track, ticks as f64 / 10_000_000.0));
                        }
                    }
                    Ok(())
                },
            ),
        )?);
        media.controls.SetIsEnabled(false)?;
        Ok(media)
    }

    fn update(&mut self, player: &PlayerView) -> Result<()> {
        let next = player.track_id.map(|id| Metadata {
            stamp: (player.queue_account_revision, id),
            title: player.title.clone(),
            artist: player.artist.clone(),
            album: player.album.clone(),
            cover_url: player.cover_url.clone(),
        });
        let changed_track = self.metadata != next;
        if changed_track {
            self.artwork_task = None;
            self.artwork_id = None;
            let updater = self.controls.DisplayUpdater()?;
            updater.ClearAll()?;
            if let Some(metadata) = &next {
                updater.SetType(MediaPlaybackType::Music)?;
                let music = updater.MusicProperties()?;
                music.SetTitle(&HSTRING::from(&metadata.title))?;
                music.SetArtist(&HSTRING::from(&metadata.artist))?;
                music.SetAlbumTitle(&HSTRING::from(
                    metadata.album.as_deref().unwrap_or_default(),
                ))?;
            }
            updater.Update()?;
            self.controls.SetIsEnabled(next.is_some())?;
            self.metadata = next;
            self.last_timeline = None;
        }
        if let Ok(mut seek_track) = self.seek_track.lock() {
            *seek_track = self
                .metadata
                .as_ref()
                .filter(|_| {
                    player.duration_ms > 0
                        && matches!(player.state, PlaybackState::Playing | PlaybackState::Paused)
                })
                .map(|metadata| metadata.stamp);
        }
        if self.metadata.is_none() {
            self.state = None;
            return self.controls.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        }

        let status = match player.state {
            PlaybackState::Playing => MediaPlaybackStatus::Playing,
            PlaybackState::Paused => MediaPlaybackStatus::Paused,
            PlaybackState::Loading => MediaPlaybackStatus::Changing,
            PlaybackState::Idle | PlaybackState::Stopped | PlaybackState::Failed => {
                MediaPlaybackStatus::Stopped
            }
        };
        let state = (status, player.can_go_previous, player.can_go_next);
        let changed_state = changed_track || self.state != Some(state);
        if changed_state {
            self.controls.SetIsPlayEnabled(true)?;
            self.controls
                .SetIsPauseEnabled(status == MediaPlaybackStatus::Playing)?;
            self.controls
                .SetIsStopEnabled(status != MediaPlaybackStatus::Stopped)?;
            self.controls.SetIsPreviousEnabled(state.1)?;
            self.controls.SetIsNextEnabled(state.2)?;
            self.controls.SetPlaybackStatus(status)?;
            self.state = Some(state);
        }
        let tick = (player.position, player.duration_ms);
        if changed_state || self.last_timeline != Some(tick) {
            let duration = player.duration_ms;
            let position = player.position_ms.min(duration);
            self.timeline.SetStartTime(span(0))?;
            self.timeline.SetMinSeekTime(span(0))?;
            self.timeline.SetEndTime(span(duration))?;
            self.timeline.SetMaxSeekTime(span(duration))?;
            self.timeline.SetPosition(span(position))?;
            self.controls.UpdateTimelineProperties(&self.timeline)?;
            self.last_timeline = Some(tick);
        }
        Ok(())
    }
}

impl Drop for WindowsMedia {
    fn drop(&mut self) {
        self.artwork_task = None;
        if let Some(token) = self.seek_token {
            let _ = self.controls.RemovePlaybackPositionChangeRequested(token);
        }
        if let Some(token) = self.button_token {
            let _ = self.controls.RemoveButtonPressed(token);
        }
        let _ = self.controls.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        let _ = self.controls.SetIsEnabled(false);
    }
}

impl MusicApp {
    pub(super) fn init_windows_media(&mut self, window: &Window, cx: &mut Context<Self>) {
        let commands = self.service.commands.clone();
        match WindowsMedia::new(window, move |command| {
            let _ = commands.send(command);
        }) {
            Ok(media) => {
                self.windows_media = Some(media);
                self.update_windows_media(cx);
            }
            Err(error) => eprintln!("Windows 系统媒体控制初始化失败：{}", error.code()),
        }
    }

    pub(super) fn update_windows_media(&mut self, cx: &mut Context<Self>) {
        if let (Some(media), Some(player)) = (self.windows_media.as_mut(), self.player.as_ref()) {
            let _ = media.update(player);
        }
        self.update_media_artwork(cx);
    }

    pub(super) fn update_media_artwork(&mut self, cx: &mut Context<Self>) {
        let Some(media) = self.windows_media.as_mut() else {
            return;
        };
        let Some(metadata) = &media.metadata else {
            return;
        };
        let Some(cover) = metadata
            .cover_url
            .as_deref()
            .and_then(|url| self.artwork.borrow().cached(url))
        else {
            return;
        };
        let id = cover.id();
        if media.artwork_id == Some(id) {
            return;
        }
        media.artwork_id = Some(id);
        let stamp = metadata.stamp;
        media.artwork_task = Some(cx.spawn(async move |view, cx| {
            let Ok(art) = artwork_reference(&cover).await else {
                return;
            };
            let _ = view.update(cx, |view, _| {
                if let Some(media) = &view.windows_media
                    && media.artwork_id == Some(id)
                    && media
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.stamp == stamp)
                    && let Ok(updater) = media.controls.DisplayUpdater()
                    && updater.SetThumbnail(&art).is_ok()
                {
                    let _ = updater.Update();
                }
            });
        }));
    }
}

async fn artwork_reference(image: &Image) -> Result<RandomAccessStreamReference> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(&image.bytes)?;
    let buffer = writer.DetachBuffer()?;
    let stream = InMemoryRandomAccessStream::new()?;
    let written = stream.WriteAsync(&buffer)?.await?;
    if written as usize != image.bytes.len() {
        return Err(Error::from_hresult(E_FAIL));
    }
    stream.Seek(0)?;
    RandomAccessStreamReference::CreateFromStream(&stream)
}

fn span(milliseconds: u64) -> TimeSpan {
    TimeSpan {
        Duration: milliseconds.saturating_mul(10_000).min(i64::MAX as u64) as i64,
    }
}

#[cfg(test)]
#[path = "windows_media_tests.rs"]
mod tests;
