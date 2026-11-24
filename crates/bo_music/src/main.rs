//! GPUI desktop shell.
mod account_page;
mod appearance;
mod artwork_cache;
mod catalog_views;
mod color_wheel;
mod comment_image;
mod desktop_lyrics;
mod floating_windows;
mod input;
mod lyric_page;
mod lyric_poster;
mod mini_player;
mod month_page;
mod mv_page;
mod playlist_ui;
mod search_view;
mod shell_browse;
mod shell_browse_rows;
mod shell_comments;
mod shell_content;
mod shell_effects;
mod shell_events;
mod shell_header;
mod shell_lyrics;
mod shell_navigation;
mod shell_overlays;
mod shell_player_bar;
mod shell_playlist_form;
mod shell_quality;
mod shell_queue;
mod shell_render;
mod shell_state;
mod shortcut_page;
mod ui;
#[cfg(windows)]
mod windows_media;
#[cfg(windows)]
mod windows_tray;

use artwork_cache::ArtworkCache;
use bo_music_app::batch_playlist::BatchProgress;
use bo_music_app::browse::{BrowseContent, BrowseView, CollectionState, DetailView, SearchKind};
use bo_music_app::desktop_service::{
    AccountEntry, AudioEffectAction, AuthView, BatchSource, Command, Event, LyricsView, PlayerView,
    PlaylistMutationResult, PlaylistPickerView, QrView, SearchSection, Service,
};
use bo_music_app::home::HomeView;
use bo_music_app::library::{LibrarySort, LibraryView};
use bo_music_app::likes::{LikeState, LikeView};
use bo_music_app::playback::QueueEdit;
use bo_music_app::playlist_cover::{CoverCrop, CoverEdit};
use bo_music_domain::comments::CommentSort;
use bo_music_domain::discovery::HomeContent;
use bo_music_domain::track::Track;
use bo_music_playback::mpv::PlaybackState;
use gpui::{
    App, Application, Bounds, ClipboardItem, Context, Entity, Focusable, Image, ImageFormat,
    KeyBinding, MouseButton, PathPromptOptions, ScrollHandle, Window, WindowBounds, WindowOptions,
    actions, canvas, div, prelude::*, px, size,
};
use input::TextInput;
use shell_state::{
    CommentPanelState, NavigationState, Page, PanelState, PlaylistDialog, PlaylistEditorState,
    SearchState, comment_draft_key,
};
use std::{
    cell::RefCell,
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

actions!(bo_music, [Search]);

enum QrDisplay {
    Idle,
    Loading,
    Ready { width: usize, modules: Vec<bool> },
    Expired,
    Failed,
}

struct MusicApp {
    panels: PanelState,
    comment_panel: CommentPanelState,
    playlist_editor: PlaylistEditorState,
    search: SearchState,
    navigation: NavigationState,
    appearance_page: appearance::AppearancePage,
    shortcuts: shortcut_page::ShortcutPage,
    playback_focus: gpui::FocusHandle,
    account_overview_generation: u64,
    account_overview: Option<Box<Result<bo_music_domain::account::AccountOverview, ()>>>,
    month_page: month_page::MonthPage,
    mv_page: mv_page::MvPage,
    artwork: RefCell<ArtworkCache>,
    home: Option<HomeView>,
    library: Option<LibraryView>,
    phone_input: Entity<TextInput>,
    code_input: Entity<TextInput>,
    preset_name_input: Entity<TextInput>,
    preset_description_input: Entity<TextInput>,
    track_picker: Option<PlaylistPickerView>,
    row_actions: Option<(BatchSource, usize, i64)>,
    batch_selection: HashSet<(usize, i64)>,
    batch_generation: Option<BatchSource>,
    history_generation: u64,
    batch_remove_confirm: bool,
    batch_progress: Option<BatchProgress>,
    clear_queue_pending: Option<(u64, u64)>,
    service: Service,
    mini_player: Option<gpui::WindowHandle<mini_player::MiniPlayer>>,
    desktop_lyrics: Option<gpui::WindowHandle<desktop_lyrics::DesktopLyrics>>,
    floating_windows: floating_windows::FloatingPreferences,
    #[cfg(windows)]
    legacy_floating_windows: bo_music_app::desktop_service::LegacyFloatingWindows,
    #[cfg(windows)]
    desktop_lyrics_locked: bool,
    recent_tracks: Vec<Track>,
    recent_loading: bool,
    recent_error: Option<String>,
    status: String,
    player: Option<PlayerView>,
    #[cfg(windows)]
    windows_media: Option<windows_media::WindowsMedia>,
    #[cfg(windows)]
    windows_tray: Option<windows_tray::WindowsTray>,
    #[cfg(target_os = "linux")]
    linux_tray_available: std::rc::Rc<std::cell::Cell<bool>>,
    like: LikeView,
    lyrics: LyricsView,
    lyric_scroll: ScrollHandle,
    lyric_page: lyric_page::LyricPage,
    lyric_poster: lyric_poster::PosterPage,
    lyric_line: Option<usize>,
    manual_scroll_until: Option<Instant>,
    auth: AuthView,
    remembered_accounts: Vec<AccountEntry>,
    account_cleanup_pending: bool,
    forget_pending: Option<String>,
    adding_account: bool,
    show_account: bool,
    show_qr: bool,
    qr: QrDisplay,
}

impl MusicApp {
    fn new(service: Service, cx: &mut Context<Self>) -> Self {
        let input = cx.new(TextInput::new);
        cx.observe(&input, |this, _, cx| this.search_input_changed(cx))
            .detach();
        let phone_input = cx.new(|cx| TextInput::with_placeholder(cx, "手机号"));
        let code_input = cx.new(|cx| TextInput::with_placeholder(cx, "短信验证码"));
        let preset_name_input = cx.new(|cx| TextInput::with_placeholder(cx, "自定义预设名称"));
        let preset_description_input = cx.new(|cx| TextInput::with_placeholder(cx, "说明（可选）"));
        let playlist_name_input = cx.new(|cx| TextInput::with_placeholder(cx, "歌单名称"));
        let playlist_description_input = cx.new(|cx| TextInput::with_placeholder(cx, "歌单简介"));
        let comment_input = cx.new(|cx| TextInput::with_placeholder(cx, "写评论或回复"));
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                if entity
                    .update(cx, |this, cx| {
                        let mut changed = false;
                        while let Ok(event) = this.service.events.try_recv() {
                            this.accept_event(event, cx);
                            changed = true;
                        }
                        let followed = this.follow_lyrics();
                        if changed || followed {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            panels: PanelState::default(),
            comment_panel: CommentPanelState::new(comment_input),
            playlist_editor: PlaylistEditorState::new(
                playlist_name_input,
                playlist_description_input,
                cx.focus_handle(),
            ),
            search: SearchState::new(input),
            navigation: NavigationState::default(),
            appearance_page: appearance::AppearancePage::new(cx),
            shortcuts: shortcut_page::ShortcutPage::new(cx),
            playback_focus: cx.focus_handle(),
            #[cfg(windows)]
            legacy_floating_windows: Default::default(),
            account_overview_generation: 0,
            account_overview: None,
            month_page: month_page::MonthPage::default(),
            mv_page: mv_page::MvPage::new(cx),
            artwork: RefCell::new(ArtworkCache::default()),
            home: None,
            library: None,
            phone_input,
            code_input,
            preset_name_input,
            preset_description_input,
            track_picker: None,
            row_actions: None,
            batch_selection: HashSet::new(),
            batch_generation: None,
            history_generation: 0,
            batch_remove_confirm: false,
            batch_progress: None,
            clear_queue_pending: None,
            service,
            mini_player: None,
            desktop_lyrics: None,
            #[cfg(windows)]
            windows_media: None,
            #[cfg(windows)]
            windows_tray: None,
            #[cfg(target_os = "linux")]
            linux_tray_available: Default::default(),
            floating_windows: Default::default(),
            #[cfg(windows)]
            desktop_lyrics_locked: false,
            recent_tracks: Vec::new(),
            recent_loading: false,
            recent_error: None,
            status: "正在启动…".into(),
            player: None,
            like: LikeView::default(),
            lyrics: LyricsView::Empty,
            lyric_scroll: ScrollHandle::new(),
            lyric_page: lyric_page::LyricPage::new(cx),
            lyric_poster: lyric_poster::PosterPage::new(cx),
            lyric_line: None,
            manual_scroll_until: None,
            auth: AuthView {
                account: None,
                busy: true,
                message: "正在启动…".into(),
            },
            remembered_accounts: Vec::new(),
            account_cleanup_pending: false,
            forget_pending: None,
            adding_account: false,
            show_account: false,
            show_qr: false,
            qr: QrDisplay::Idle,
        }
    }

    fn lyric_seek_stamp(&self) -> Option<(u64, i64)> {
        let player = self.player.as_ref()?;
        Some((player.queue_account_revision, player.track_id?))
    }

    fn seek_lyric(&self, stamp: Option<(u64, i64)>, target: f64) {
        if let Some((account, track)) = stamp
            && stamp == self.lyric_seek_stamp()
        {
            let _ = self
                .service
                .commands
                .send(Command::SeekTo(account, track, target));
        }
    }

    fn follow_lyrics(&mut self) -> bool {
        let LyricsView::Ready(document) = &self.lyrics else {
            return false;
        };
        let position = self.player.as_ref().map_or(0, |player| player.position_ms);
        let current = document
            .line_at(position)
            .and_then(|line| document.source_line(line));
        let resumed = self
            .manual_scroll_until
            .is_some_and(|until| Instant::now() >= until);
        if resumed {
            self.manual_scroll_until = None;
        }
        if current != self.lyric_line || resumed {
            self.lyric_line = current;
            if self.panels.show_lyrics
                && !self.panels.show_effects
                && !self.panels.show_quality
                && self
                    .manual_scroll_until
                    .is_none_or(|until| Instant::now() >= until)
                && let Some(index) = current
            {
                self.lyric_scroll.scroll_to_item(index);
            }
        }
        self.lyric_page.follow(current) || resumed
    }
}

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn quality_name(quality: bo_music_domain::track::AudioQuality) -> &'static str {
    use bo_music_domain::track::AudioQuality;
    match quality {
        AudioQuality::Standard => "标准",
        AudioQuality::High => "高品",
        AudioQuality::Lossless => "无损",
    }
}

fn activate_main_window(window: &Window) {
    #[cfg(windows)]
    windows_tray::restore_hidden_window(window);
    window.activate_window();
}

fn main() {
    let primary = match bo_music_app::instance::PrimaryInstance::start() {
        Ok(Some(primary)) => primary,
        Ok(None) => return,
        Err(error) => {
            eprintln!("无法启动bo-music：{error}");
            std::process::exit(1);
        }
    };
    let inbox = primary.activation_inbox();
    let (service, placement) = Service::start();
    Application::new()
        .with_assets(ui::Assets)
        .run(move |cx: &mut App| {
            appearance::init(cx);
            TextInput::register_bindings(cx);
            lyric_page::register_bindings(cx);
            lyric_poster::register_bindings(cx);
            mv_page::register_bindings(cx);
            cx.bind_keys([KeyBinding::new("enter", Search, None)]);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let fallback = Bounds::centered(None, size(px(1100.), px(720.)), cx);
            let minimum = size(px(780.), px(540.));
            let (bounds, display_id) =
                floating_windows::restore_placement(placement.main, fallback, None, minimum, cx);
            #[cfg(windows)]
            let (bounds, display_id) = if placement.main.is_none() {
                floating_windows::restore_legacy_placement(placement.legacy_main, minimum, cx)
                    .unwrap_or((bounds, display_id))
            } else {
                (bounds, display_id)
            };
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_min_size: Some(minimum),
                        display_id,
                        app_id: Some("bo_music".into()),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some("bo-music".into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|cx| MusicApp::new(service, cx)),
                )
                .expect("failed to open main window");
            window
                .update(cx, |view, window, cx| {
                    window.focus(&view.search.input.focus_handle(cx));
                    floating_windows::observe_main_placement(window, cx);
                    appearance::observe(window, cx);
                    #[cfg(windows)]
                    {
                        view.init_windows_media(window, cx);
                        view.init_windows_tray(window, cx);
                    }
                    #[cfg(target_os = "linux")]
                    {
                        let available = view.linux_tray_available.clone();
                        window.on_window_should_close(cx, move |window, _| {
                            if available.get() {
                                window.minimize_window();
                                false
                            } else {
                                true
                            }
                        });
                    }
                    cx.activate(true);
                })
                .expect("failed to initialize main window");
            cx.spawn(async move |cx| {
                loop {
                    let inbox = inbox.clone();
                    let requested = cx
                        .background_executor()
                        .spawn(async move { inbox.take_request() })
                        .await;
                    if matches!(requested, Ok(true))
                        && window
                            .update(cx, |_, window, _| activate_main_window(window))
                            .is_err()
                    {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                }
            })
            .detach();
        });
    drop(primary);
}
