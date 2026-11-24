//! A dedicated Tokio thread owns networking and playback; GPUI never blocks on either.
use crate::month_reports::{self, ReportCache};
use crate::{
    artwork::ArtworkFetcher,
    auth::{self, Account, AuthError},
    batch_playlist::{self, BatchJob, BatchProgress},
    browse::{Browser, SearchKind},
    comments::{Comments, PublishOutcome},
    home::Home,
    library::MusicLibrary,
    likes::LikedSongs,
    mv::MvController,
    playback::{
        ApplyOutcome, PlaybackCoordinator, PlaybackReport, Prefetcher, QualityOutcome, QueueEdit,
        RecoveryOutcome, ResolutionTicket,
    },
};
use bo_music_api::{
    account::{create_report_playlist_at_revision, overview_at_revision},
    catalog::track_at_revision,
    library as library_api,
    login::{self, ExchangeOutcome, QrScanStatus, SmsOutcome},
    lyrics,
    playlist::{self, PlaylistEdit},
    search::{
        comprehensive_at_revision, hot_words, search_cursor, search_tracks, suggestions_at_revision,
    },
    session::Session,
    sharing::{self, ShareOutcome, ShareTarget},
    telemetry::{self, PlayOperation},
    transport::{Transport, TransportOptions},
};
use bo_music_domain::account::{MonthPeriod, MonthReport, PlayRankingEntry};
use bo_music_domain::audio_effects::{AudioEffects, CustomPreset, MAX_CUSTOM_PRESETS, PRESETS};
use bo_music_domain::catalog::{CategoryGroup, Playlist};
use bo_music_domain::paging::PagedCursor;
use bo_music_domain::queue::PlayMode;
use bo_music_domain::track::{AudioQuality, Track};
use bo_music_playback::mpv::{MpvOptions, MpvPlayer, PlaybackState};
#[cfg(windows)]
use bo_music_storage::migration;
use bo_music_storage::{
    atomic,
    credentials::{CredentialError, CredentialStore, OsSecretBackend, SecretBackend},
    device,
    paths::DataPaths,
    queue::QueueStore,
    settings::SettingsStore,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc,
    },
    time::Duration,
};
use tokio::sync::mpsc;

pub use crate::month_poster::{MonthPosterKind, MonthPosterStyle};
pub use crate::mv::{MvAction, MvQuality, MvView};
pub use bo_music_api::search::SearchSection;

pub use bo_music_storage::settings::{AppearanceSettings, ThemeMode};
pub use bo_music_storage::settings::{FloatingBounds, FloatingWindows};
#[cfg(windows)]
pub use bo_music_storage::settings::{LegacyFloatingWindows, PhysicalWindowBounds};
pub use bo_music_storage::settings::{LyricPosterStyle, PosterTheme};
pub use bo_music_storage::shortcuts::{
    Action as ShortcutAction, Gesture as ShortcutGesture, Shortcuts,
};

mod messages;
pub use messages::*;

#[macro_use]
mod handlers;
mod audio;
mod batch;
mod history;
mod runtime;
use audio::{audio_filter, changed_audio_effects};
use batch::{selected_tracks, start_batch, start_liked_batch};
use history::{HistoryWork, SearchHistoryAction, history_worker};

struct PlaylistPickerState {
    revision: u64,
    generation: u64,
    track_ids: Vec<i64>,
    view: PlaylistPickerView,
}

type SharedSecrets = Arc<dyn SecretBackend + Send + Sync>;

fn normalize_mobile(raw: &str) -> Option<String> {
    let mut digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if (raw.trim_start().starts_with("+86") || digits.len() > 11) && digits.starts_with("86") {
        digits.drain(..2);
    }
    let bytes = digits.as_bytes();
    (bytes.len() == 11 && bytes[0] == b'1' && (b'3'..=b'9').contains(&bytes[1])).then_some(digits)
}

fn auth_view(
    events: &std_mpsc::Sender<Event>,
    account: &Option<Account>,
    busy: bool,
    message: &str,
) {
    let _ = events.send(Event::Auth(AuthView {
        account: account.clone(),
        busy,
        message: message.into(),
    }));
}

pub struct Service {
    pub commands: mpsc::UnboundedSender<Command>,
    pub events: std_mpsc::Receiver<Event>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[derive(Default)]
pub struct StartupPlacement {
    pub main: Option<FloatingBounds>,
    #[cfg(windows)]
    pub legacy_main: Option<PhysicalWindowBounds>,
}

fn mpv_library() -> PathBuf {
    if let Some(path) = std::env::var_os("BO_MUSIC_LIBMPV") {
        return path.into();
    }
    #[cfg(windows)]
    {
        if let Some(path) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("libmpv-2.dll")))
            && path.is_file()
        {
            return path;
        }
        for path in ["libmpv/libmpv-2.dll", "../libmpv/libmpv-2.dll"] {
            if std::path::Path::new(path).is_file() {
                return path.into();
            }
        }
        "libmpv-2.dll".into()
    }
    #[cfg(not(windows))]
    {
        "libmpv.so.2".into()
    }
}

fn video_library(audio_library: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("BO_MUSIC_VIDEO_LIBMPV").filter(|path| !path.is_empty()) {
        return path.into();
    }
    #[cfg(windows)]
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|parent| parent.join("libmpv-video-2.dll"))
        })
        .filter(|path| path.is_file())
    {
        return path;
    }
    audio_library.to_path_buf()
}

fn initialize_paths(override_paths: Option<DataPaths>) -> Result<DataPaths, String> {
    if let Some(paths) = override_paths {
        std::fs::create_dir_all(paths.root()).map_err(|_| "无法创建数据目录".to_owned())?;
        return Ok(paths);
    }
    let paths = DataPaths::platform_default().map_err(|_| "无法确定数据目录".to_owned())?;
    std::fs::create_dir_all(paths.root()).map_err(|_| "无法创建数据目录".to_owned())?;
    #[cfg(windows)]
    {
        let marker = paths.root().join("legacy-import-complete");
        let settings_marker = paths.root().join("legacy-settings-import-complete");
        if let Some(old) = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("Bowen"))
            && old.is_dir()
        {
            if !marker.exists() {
                migration::import_legacy(&old, &paths)
                    .map_err(|_| "旧版数据迁移失败，请检查数据目录权限".to_owned())?;
                atomic::create(&marker, b"1").map_err(|_| "无法记录旧版数据迁移状态".to_owned())?;
                atomic::create(&settings_marker, b"1")
                    .map_err(|_| "无法记录旧版设置迁移状态".to_owned())?;
            } else if !settings_marker.exists() {
                migration::import_standalone_settings(&old, &paths)
                    .map_err(|_| "旧版设置迁移失败，请检查数据目录权限".to_owned())?;
                atomic::create(&settings_marker, b"1")
                    .map_err(|_| "无法记录旧版设置迁移状态".to_owned())?;
            }
        }
    }
    Ok(paths)
}

fn launch(
    ticket: Option<ResolutionTicket>,
    commands: &mpsc::UnboundedSender<Command>,
    task: &mut Option<tokio::task::JoinHandle<()>>,
    prefetch: &mut Prefetcher,
) {
    if let Some(ticket) = ticket {
        if let Some(previous) = task.take() {
            previous.abort();
        }
        let cached = prefetch.take_for(&ticket);
        let commands = commands.clone();
        *task = Some(tokio::spawn(async move {
            let _ = commands.send(Command::Resolved(
                ticket.resolve_with_prefetch(cached).await,
            ));
        }));
    }
}

fn schedule_reports(
    transport: &Arc<Transport>,
    reports: Vec<PlaybackReport>,
    tasks: &mut tokio::task::JoinSet<()>,
) {
    while tasks.try_join_next().is_some() {}
    for report in reports {
        let revision = match &report {
            PlaybackReport::Started { revision, .. }
            | PlaybackReport::Ended { revision, .. }
            | PlaybackReport::Collection { revision, .. } => *revision,
        };
        let session = transport.session().snapshot();
        if tasks.len() >= 16 || !session.is_authenticated() || session.revision != revision {
            continue;
        }
        let transport = Arc::clone(transport);
        tasks.spawn(async move {
            let request = async {
                match report {
                    PlaybackReport::Started { music_id, revision } => {
                        let _ = telemetry::song_history_for_revision(
                            &transport,
                            music_id,
                            Some(revision),
                        )
                        .await;
                    }
                    PlaybackReport::Ended {
                        music_id,
                        duration,
                        position,
                        completed,
                        revision,
                    } => {
                        let operation = if completed {
                            Some(PlayOperation::Complete)
                        } else {
                            PlayOperation::for_position(duration as u32, position as u32)
                        };
                        if let Some(operation) = operation {
                            let _ = telemetry::play_record_for_revision(
                                &transport,
                                music_id,
                                duration,
                                position,
                                operation,
                                Some(revision),
                            )
                            .await;
                        }
                    }
                    PlaybackReport::Collection { origin, revision } => {
                        let _ = telemetry::collection_history_for_revision(
                            &transport,
                            origin.kind,
                            origin.id,
                            origin.source_type,
                            Some(revision),
                        )
                        .await;
                    }
                }
            };
            let _ = tokio::time::timeout(Duration::from_secs(5), request).await;
        });
    }
}

fn launch_search(
    transport: Arc<Transport>,
    query: String,
    mut cursor: PagedCursor,
    generation: u64,
    sender: mpsc::UnboundedSender<Command>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let result = search_tracks(&transport, &query, &mut cursor)
            .await
            .map(|page| page.items)
            .map_err(|_| "搜索失败，请检查网络后重试".to_owned());
        let _ = sender.send(Command::SearchDone(generation, cursor, result));
    })
}

fn launch_qr(
    transport: Arc<Transport>,
    sender: mpsc::UnboundedSender<Command>,
    generation: u64,
    interval: Duration,
    lifetime: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let flow = async {
            let challenge = login::create_challenge(&transport).await.map_err(|_| ())?;
            if challenge.landing_url.len() > 1024 {
                return Err(());
            }
            if sender
                .send(Command::QrUpdate(
                    generation,
                    QrUpdate::Ready(challenge.landing_url.clone()),
                ))
                .is_err()
            {
                return Ok(());
            }
            loop {
                tokio::time::sleep(interval).await;
                match login::poll(&transport, &challenge).await.map_err(|_| ())? {
                    QrScanStatus::Waiting | QrScanStatus::Unknown => {}
                    QrScanStatus::Expired => {
                        let _ = sender.send(Command::QrUpdate(generation, QrUpdate::Expired));
                        return Ok(());
                    }
                    QrScanStatus::Confirmed => {
                        let outcome = login::complete_qr(&transport, &challenge)
                            .await
                            .map_err(|_| ())?;
                        let _ =
                            sender.send(Command::QrUpdate(generation, QrUpdate::Complete(outcome)));
                        return Ok(());
                    }
                }
            }
        };
        match tokio::time::timeout(lifetime, flow).await {
            Ok(Ok(())) => {}
            Ok(Err(())) => {
                let _ = sender.send(Command::QrUpdate(generation, QrUpdate::Failed));
            }
            Err(_) => {
                let _ = sender.send(Command::QrUpdate(generation, QrUpdate::Expired));
            }
        }
    })
}

fn player_view(
    player: &PlaybackCoordinator<MpvPlayer>,
    volume: f64,
    audio_effects: &AudioEffects,
) -> PlayerView {
    let state = player.snapshot();
    let track = player.queue().current();
    PlayerView {
        track_id: track.map(|item| item.id),
        cover_url: track.and_then(|item| item.cover_url.clone()),
        queue_account_revision: player.queue_account_revision(),
        queue_revision: player.queue().revision(),
        queue_reorderable: player.queue().can_reorder(),
        can_go_next: player.queue().has_next(),
        can_go_previous: player.queue().has_previous(),
        state: state.state,
        recovering: state.recovering,
        title: track.map_or_else(|| "未播放".into(), |t| t.title.clone()),
        artist: track.map_or_else(String::new, |t| t.artist_text.clone()),
        album: track.and_then(|track| track.album_name.clone()),
        position: state.position_seconds.max(0.0) as u64,
        position_ms: (state.position_seconds.max(0.0) * 1000.0) as u64,
        duration: state.duration_seconds.unwrap_or(0.0).max(0.0) as u64,
        duration_ms: (state.duration_seconds.unwrap_or(0.0).max(0.0) * 1000.0) as u64,
        queue: player
            .queue()
            .items()
            .iter()
            .map(|track| format!("{} · {}", track.title, track.artist_text))
            .collect(),
        selected: player.queue().current_index(),
        mode: player.queue().mode(),
        volume: volume.round() as u8,
        audio_effects: audio_effects.clone(),
        audio_filter_failed: state.audio_filter_failed,
        preferred_quality: player.preferred_quality(),
        current_quality: player.current_quality(),
        available_qualities: player.available_qualities(),
        changing_quality: player.changing_quality(),
        can_change_quality: player.can_change_quality(),
    }
}

#[cfg(test)]
#[path = "desktop_service_audio_effect_tests.rs"]
mod audio_effect_tests;

#[cfg(all(test, windows))]
#[path = "mv_integration_test.rs"]
mod mv_integration_test;

#[cfg(all(test, windows))]
#[path = "desktop_service_tests.rs"]
mod tests;
