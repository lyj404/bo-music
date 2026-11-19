//! A dedicated Tokio thread owns networking and playback; GPUI never blocks on either.
use crate::month_reports::{self, ReportCache, ReportResult};
use crate::{
    artwork::{ArtworkFetcher, ArtworkThumbnail},
    auth::{self, Account, AuthError},
    batch_playlist::{self, BatchJob, BatchProgress},
    browse::{
        BrowseResult, BrowseView, Browser, CollectionState, CollectionTarget, DetailResult,
        SearchKind,
    },
    comments::{CommentContext, CommentLike, Comments, CommentsView, PublishOutcome},
    home::{Home, HomeView},
    library::{LibrarySort, LibraryView, MusicLibrary},
    likes::{LikeView, LikedSnapshot, LikedSongs},
    mv::{AudioResume, MvCandidate, MvController},
    playback::{
        ApplyOutcome, CurrentQuality, PlaybackCoordinator, PlaybackReport, Prefetcher,
        QualityOutcome, QualityResult, QueueEdit, RecoveryOutcome, RecoveryResult,
        ResolutionTicket, ResolvedTicket,
    },
    playlist_cover::{CoverEdit, CoverImage},
};
use bowen_api::{
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
use bowen_domain::account::{
    AccountOverview, MonthAvailability, MonthPeriod, MonthReport, PlayRankingEntry,
};
use bowen_domain::audio_effects::{AudioEffects, CustomPreset, MAX_CUSTOM_PRESETS, PRESETS};
use bowen_domain::catalog::{Album, Artist, CategoryGroup, LibraryGroup, Playlist};
use bowen_domain::comments::{CommentPage, CommentSort};
use bowen_domain::discovery::{AiPlaylist, Chart, HomeFeed, HomeModule};
use bowen_domain::lyrics::LyricDocument;
use bowen_domain::paging::PagedCursor;
use bowen_domain::queue::PlayMode;
use bowen_domain::track::{AudioQuality, Track};
use bowen_playback::mpv::{MpvOptions, MpvPlayer, PlaybackState};
#[cfg(windows)]
use bowen_storage::migration;
use bowen_storage::{
    atomic,
    credentials::{CredentialError, CredentialStore, OsSecretBackend, SecretBackend},
    device,
    history::HistoryStore,
    paths::DataPaths,
    queue::QueueStore,
    search_history::SearchHistoryStore,
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
pub use bowen_api::search::SearchSection;

pub use bowen_storage::settings::{AppearanceSettings, ThemeMode};
pub use bowen_storage::settings::{FloatingBounds, FloatingWindows};
#[cfg(windows)]
pub use bowen_storage::settings::{LegacyFloatingWindows, PhysicalWindowBounds};
pub use bowen_storage::settings::{LyricPosterStyle, PosterTheme};
pub use bowen_storage::shortcuts::{
    Action as ShortcutAction, Gesture as ShortcutGesture, Shortcuts,
};

// Command groups share the worker's state and can skip the rest of its loop.
// Assemble a single exhaustive match so new commands must have a handler.
macro_rules! collect_command_arms {
    ($command:ident [$($arms:tt)*] []) => { match $command { $($arms)* } };
    ($command:ident $arms:tt [$handler:ident [$($args:tt)*] $($rest:tt)*]) => {
        $handler!($command $arms [$($rest)*]; $($args)*)
    };
}

#[macro_use]
#[path = "service_settings_and_reports.rs"]
mod settings_and_reports_routes;
#[macro_use]
#[path = "service_accounts.rs"]
mod accounts_routes;
#[macro_use]
#[path = "service_search_and_comments.rs"]
mod search_and_comments_routes;
#[macro_use]
#[path = "service_playlist_edits.rs"]
mod playlist_edits_routes;
#[macro_use]
#[path = "service_catalog.rs"]
mod catalog_routes;
#[macro_use]
#[path = "service_playback.rs"]
mod playback_routes;

#[path = "service_audio.rs"]
mod audio;
#[path = "service_batch.rs"]
mod batch;
#[path = "service_runtime.rs"]
mod runtime;
use audio::{audio_filter, changed_audio_effects};
use batch::{selected_tracks, start_batch, start_liked_batch};

pub enum Command {
    SaveAppearance(AppearanceSettings),
    SaveMainWindow(FloatingBounds),
    SaveLyricPosterStyle(LyricPosterStyle),
    SaveShortcuts(Shortcuts),
    ToggleMute,
    OpenMv(u64, i64),
    ControlMv(MvAction),
    CloseMv,
    MvLoaded(u64, Result<MvCandidate, &'static str>),
    MvResumeAudio(AudioResume),
    SaveFloatingWindows(FloatingWindows),
    LoadMonthReport(u64, MonthPeriod),
    RenderMonthPoster(u64, u64, MonthPosterStyle),
    MonthPosterDone(u64, u64, u64, Result<Vec<u8>, ()>),
    SaveMonthPoster(u64, u64, PathBuf),
    MonthPosterSaved(u64, u64, u64, bool),
    ProbeMonthReports(u64, MonthPeriod, MonthPeriod),
    ProbeMonthReportResult(u64, u64, MonthPeriod, ReportResult),
    ProbeMonthReportsFinished(u64, u64),
    CloseMonthReport(u64),
    MonthReportDone(u64, u64, MonthPeriod, Result<Option<Box<MonthReport>>, ()>),
    CreateMonthPlaylist(u64, MonthPeriod),
    MonthPlaylistDone(u64, MonthPeriod, bool),
    PlayMonthTrack(u64, i64),
    MonthTrackDone(u64, u64, Option<Box<Track>>),
    LoadAccountOverview(u64),
    AccountOverviewDone(u64, u64, Box<Result<AccountOverview, ()>>),
    PlayWeeklyTrack(u64, usize, i64),
    LoadArtwork(String),
    LoadCommentImage(u64, u64, i64, bool),
    CommentImageDone(u64, u64, u64, i64, Result<Vec<u8>, ()>),
    SaveCommentImage(u64, PathBuf),
    SearchSuggestions(u64, String),
    LoadSearchHotWords,
    LoadSearchHistory,
    ClearSearchHistory,
    SearchHistoryDone(u64, Result<Vec<String>, String>),
    SearchOverview(u64, String),
    SearchOverviewDone(u64, u64, Result<Vec<SearchSection>, ()>),
    SelectSearchOverview(u64, usize, usize),
    StartQr,
    CancelQr,
    QrUpdate(u64, QrUpdate),
    RequestSms(String),
    SmsDone(Result<SmsOutcome, String>),
    LoginPhone(String, String),
    PhoneDone(Result<ExchangeOutcome, String>),
    SignOut,
    SwitchAccount(String),
    ForgetAccount(String),
    RetryAccountCleanup,
    RefreshAccounts,
    AccountsDone(u64, Result<AccountList, ()>),
    AuthDone(AuthOperation, Result<Option<Account>, AuthError>),
    #[cfg(windows)]
    LegacyCredentialNotice,
    Search(String),
    SearchNext,
    ShowHistory,
    PlaylistSquare,
    SelectPlaylistCategory(i64, u64),
    HistoryDone(u64, u64, Result<Vec<Track>, String>),
    HistoryRecorded(u64),
    SearchPlaylists(String),
    SearchAlbums(String),
    SearchArtists(String),
    MyPlaylists,
    Collections(SearchKind),
    LoadCollectionState,
    ToggleCollection,
    CollectionStateDone(u64, u64, CollectionTarget, CollectionState),
    CollectionWriteDone(u64, CollectionTarget, bool, bool),
    ToggleLike(i64),
    RetryLike(i64),
    LikedLoadDone(u64, u64, Result<LikedSnapshot, ()>),
    LikedWriteDone(u64, i64, bool, bool),
    StartPlaylistEdit(i64),
    LoadPlaylistCover(u64, PathBuf),
    PlaylistEditLoaded(
        u64,
        u64,
        i64,
        Result<Option<Playlist>, ()>,
        Option<Vec<CategoryGroup>>,
    ),
    CreatePlaylist(String, bool),
    SavePlaylist(i64, String, String, Vec<i32>, Option<CoverEdit>),
    DeletePlaylist(i64),
    PlaylistMutationDone(u64, Result<PlaylistMutationResult, PlaylistMutationError>),
    OpenTrackPicker(Track),
    OpenBatchPicker(BatchSource, Vec<(usize, i64)>),
    LikeSelected(BatchSource, Vec<(usize, i64)>),
    AddPickedTracks(u64, i64),
    RemoveSelectedTracks(i64, u64, Vec<(usize, i64)>),
    CancelBatch,
    BatchProgress(u64, BatchProgress),
    BatchDone(u64, i64, BatchAction, Option<u64>, BatchProgress),
    BatchPreparationFailed(u64, &'static str),
    CloseTrackPicker,
    TrackPickerLoaded(u64, u64, Result<Vec<Playlist>, ()>),
    AddTrackToPlaylist(i64, i64),
    RemovePlaylistTrack(i64, usize, i64, u64),
    TrackMutationDone(u64, TrackMutation, bool),
    OpenComments,
    CloseComments,
    CommentSort(CommentSort, u64),
    CommentsNext(u64),
    CommentsRefresh(u64),
    OpenCommentThread(i64, u64),
    CloseCommentThread(u64),
    ReplyToComment(i64, u64),
    ClearCommentReply(u64),
    ToggleCommentLike(i64, u64),
    PublishComment(u64, String, bool),
    CommentsPageDone(u64, CommentContext, Option<i64>, Result<CommentPage, ()>),
    CommentLikeDone(CommentLike, bool),
    CommentPublishDone(CommentContext, PublishOutcome),
    ShareCurrent(u64, u64, i64),
    ShareDetail(u64),
    ShareReportDone(u64, i64, Result<ShareOutcome, ()>),
    LoadHome,
    HomeNext,
    HomeRefresh(usize, u64),
    HomeOpenAi(usize, usize, u64),
    HomeCloseAi(u64),
    HomeOpenPlaylist(usize, usize, i64, u64),
    HomePlay(Option<usize>, usize, i64, u64),
    HomePlayAll(u64),
    HomeQueueModule(usize, u64, u64, u64),
    HomeLayoutDone(u64, u64, Result<Vec<HomeModule>, ()>),
    HomeBatchDone(
        u64,
        u64,
        usize,
        Vec<(HomeModule, Result<Option<HomeFeed>, ()>)>,
    ),
    HomeRefreshDone(u64, u64, usize, Result<Option<HomeFeed>, ()>),
    HomeAiDone(u64, u64, u64, Result<Option<AiPlaylist>, ()>),
    LoadLibrary,
    LibrarySelectGroup(usize, u64),
    LibrarySelectChild(usize, u64),
    LibrarySort(LibrarySort, u64),
    LibraryBack(u64),
    LibraryNext(u64),
    LibraryOpenAlbum(Option<usize>, i64, u64),
    LibraryNavigationDone(u64, u64, Result<Vec<LibraryGroup>, ()>),
    LibraryAlbumsDone(u64, u64, PagedCursor, Result<Vec<Album>, ()>),
    Charts,
    OpenChart(Chart),
    OpenPlaylist(Playlist),
    OpenAlbum(Album),
    OpenArtist(Artist),
    ArtistTracks,
    ArtistAlbums,
    LoadDetails,
    DetailDone(u64, u64, Result<DetailResult, String>),
    BrowseBack,
    BrowseNext,
    PlayAllPlaylist,
    BrowseDone(u64, u64, Result<BrowseResult, String>),
    Play(Vec<Track>, usize),
    PlaySingle(Track),
    PlayBrowse(usize, i64, u64),
    Toggle,
    SetPlaying(bool),
    Stop,
    Next,
    Previous,
    EditQueue(u64, u64, QueueEdit),
    EditBrowseQueue(u64, u64, usize, i64, u64, bool),
    SeekRelative(i32),
    SeekTo(u64, i64, f64),
    SeekFromMedia(u64, i64, i64),
    SetVolume(f64),
    SetPlayMode(PlayMode),
    RaiseMainWindow,
    ExitApplication,
    VolumeDelta(i32),
    AudioEffect(AudioEffectAction),
    SetPreferredQuality(AudioQuality),
    SwitchQuality(AudioQuality),
    QualityResolved(QualityResult),
    CycleMode,
    SearchDone(u64, PagedCursor, Result<Vec<Track>, String>),
    LyricsDone(u64, Result<LyricDocument, String>),
    Resolved(ResolvedTicket),
    RecoveryResolved(RecoveryResult),
    Shutdown,
}

pub enum AudioEffectAction {
    Toggle,
    Select(String),
    AdjustBand(usize, f64),
    SelectAmbient(String),
    AdjustAmbientVolume(f64),
    CreateCustom { name: String, description: String },
    RenameCustom { name: String, description: String },
    DeleteCustom,
    ResetBands,
    Set(AudioEffects),
}

#[derive(Clone, PartialEq)]
pub struct PlayerView {
    pub track_id: Option<i64>,
    pub cover_url: Option<String>,
    pub queue_account_revision: u64,
    pub queue_revision: u64,
    pub queue_reorderable: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub state: PlaybackState,
    pub recovering: bool,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub position: u64,
    pub position_ms: u64,
    pub duration: u64,
    pub duration_ms: u64,
    pub queue: Vec<String>,
    pub selected: Option<usize>,
    pub mode: PlayMode,
    pub volume: u8,
    pub audio_effects: AudioEffects,
    pub audio_filter_failed: bool,
    pub preferred_quality: AudioQuality,
    pub current_quality: Option<CurrentQuality>,
    pub available_qualities: Vec<AudioQuality>,
    pub changing_quality: bool,
    pub can_change_quality: bool,
}

pub enum Event {
    Appearance(AppearanceSettings),
    AppearanceSaved(bool),
    LyricPosterStyle(LyricPosterStyle),
    #[cfg(windows)]
    LegacyFloatingWindows(LegacyFloatingWindows),
    Shortcuts(Shortcuts),
    ShortcutsSaved(bool),
    Mv(Option<MvView>),
    ActivateMainWindow,
    ExitApplication,
    LinuxTrayAvailable(bool),
    FloatingWindows(FloatingWindows),
    CommentImage(u64, Result<Arc<Vec<u8>>, ()>),
    CommentImageSaved(u64, bool),
    MonthPoster(u64, u64, Result<Arc<Vec<u8>>, ()>),
    MonthPosterSaved(u64, u64, bool),
    MonthReport(u64, Result<Option<Box<MonthReport>>, ()>),
    MonthAvailability(u64, MonthPeriod, MonthAvailability),
    MonthAvailabilityFinished(u64),
    MonthPlaylist(MonthPeriod, bool),
    AccountOverview(u64, Box<Result<AccountOverview, ()>>),
    SearchSuggestions(u64, Vec<String>),
    SearchHotWords(Option<Vec<String>>),
    SearchHistory(Result<Vec<String>, String>),
    SearchOverview(u64, Result<Vec<SearchSection>, ()>),
    SearchOverviewOpened(u64),
    Artwork(String, Result<ArtworkThumbnail, ()>),
    Qr(QrView),
    Auth(AuthView),
    Accounts(AccountList),
    Status(String),
    Search(SearchView),
    History(HistoryView),
    HistoryChanged,
    Browse(BrowseView),
    Lyrics(LyricsView),
    Like(LikeView),
    PlaylistEditReady(Playlist, Option<Vec<CategoryGroup>>),
    PlaylistCoverPreview(u64, Result<Arc<CoverImage>, ()>),
    PlaylistEditFailed,
    PlaylistMutationBusy(bool),
    PlaylistMutationFinished(Option<PlaylistMutationResult>),
    TrackPicker(Option<PlaylistPickerView>),
    BatchProgress(Option<BatchProgress>),
    Comments(Box<CommentsView>),
    CommentPostResult(u64, bool),
    CopyShareLink(String),
    Home(HomeView),
    HomePlaylistOpened,
    Library(LibraryView),
    LibraryAlbumOpened,
    Player(PlayerView),
}

#[derive(Clone)]
pub enum PlaylistMutationResult {
    Created {
        id: i64,
        name: String,
        private: bool,
    },
    Edited {
        id: i64,
        name: String,
        description: String,
        cover_url: Option<String>,
    },
    Deleted {
        id: i64,
    },
}

pub enum PlaylistMutationError {
    InvalidCover,
    Uncertain,
}

#[derive(Clone)]
pub struct PlaylistPickerView {
    pub track_id: i64,
    pub track_title: String,
    pub generation: u64,
    pub track_count: usize,
    pub playlists: Vec<Playlist>,
    pub loading: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchSource {
    Browse(u64),
    Search(u64),
    History(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchAction {
    Add,
    Remove,
    Like,
}

struct PlaylistPickerState {
    revision: u64,
    generation: u64,
    track_ids: Vec<i64>,
    view: PlaylistPickerView,
}

#[derive(Clone, Copy)]
pub struct TrackMutation {
    playlist_id: i64,
    track_id: i64,
    add: bool,
    picker_generation: Option<u64>,
}

/// QR contents include a short-lived login secret and must not be logged.
pub enum QrView {
    Idle,
    Loading,
    Ready(String),
    Expired,
    Failed,
}

pub enum QrUpdate {
    Ready(String),
    Expired,
    Failed,
    Complete(ExchangeOutcome),
}

#[derive(Clone, Copy)]
pub enum AuthOperation {
    Restore,
    Login,
    SignOut,
    Expired,
    Switch,
    Forget,
    Cleanup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountEntry {
    pub uid: String,
    pub nickname: Option<String>,
    pub stale: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountList {
    pub entries: Vec<AccountEntry>,
    pub cleanup_pending: bool,
}

#[derive(Clone)]
pub struct AuthView {
    pub account: Option<Account>,
    pub busy: bool,
    pub message: String,
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

pub enum LyricsView {
    Empty,
    Loading,
    Ready(LyricDocument),
    Failed,
}

pub struct SearchView {
    pub generation: u64,
    pub tracks: Vec<Track>,
    pub has_more: bool,
    pub loading: bool,
}

pub struct HistoryView {
    pub generation: u64,
    pub tracks: Vec<Track>,
    pub loading: bool,
    pub error: Option<String>,
}

enum HistoryWork {
    Search {
        scope: String,
        revision: u64,
        action: SearchHistoryAction,
    },
    Load {
        scope: String,
        generation: u64,
        revision: u64,
    },
    Record {
        scope: String,
        track: Box<Track>,
        revision: u64,
    },
}

enum SearchHistoryAction {
    Load,
    Record(String),
    Clear,
}

fn history_worker(
    paths: DataPaths,
    commands: mpsc::UnboundedSender<Command>,
    events: std_mpsc::Sender<Event>,
) -> (std_mpsc::Sender<HistoryWork>, std::thread::JoinHandle<()>) {
    let (sender, receiver) = std_mpsc::channel();
    let worker = std::thread::spawn(move || {
        let store = HistoryStore::new(paths.clone());
        let search_store = SearchHistoryStore::new(paths);
        while let Ok(work) = receiver.recv() {
            match work {
                HistoryWork::Search {
                    scope,
                    revision,
                    action,
                } => {
                    let result = match action {
                        SearchHistoryAction::Load => {
                            search_store.load(&scope).map_err(|_| "无法读取搜索历史")
                        }
                        SearchHistoryAction::Record(keyword) => search_store
                            .record(&scope, keyword)
                            .map_err(|_| "无法保存搜索历史"),
                        SearchHistoryAction::Clear => search_store
                            .clear(&scope)
                            .map(|()| Vec::new())
                            .map_err(|_| "无法清空搜索历史"),
                    };
                    let _ = commands.send(Command::SearchHistoryDone(
                        revision,
                        result.map_err(str::to_owned),
                    ));
                }
                HistoryWork::Load {
                    scope,
                    generation,
                    revision,
                } => {
                    let result = store
                        .load(&scope)
                        .map_err(|_| "读取最近播放失败".to_owned());
                    let _ = commands.send(Command::HistoryDone(generation, revision, result));
                }
                HistoryWork::Record {
                    scope,
                    track,
                    revision,
                } => {
                    if store.record(&scope, &track).is_err() {
                        let _ =
                            events.send(Event::Status("保存最近播放失败，请检查数据目录".into()));
                    } else {
                        let _ = commands.send(Command::HistoryRecorded(revision));
                    }
                }
            }
        }
    });
    (sender, worker)
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
    if let Some(path) = std::env::var_os("BOWEN_LIBMPV") {
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
    if let Some(path) = std::env::var_os("BOWEN_VIDEO_LIBMPV").filter(|path| !path.is_empty()) {
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
