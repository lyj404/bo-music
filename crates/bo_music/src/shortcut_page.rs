use super::{MusicApp, PlaylistDialog};
use bo_music_app::desktop_service::{Command, ShortcutAction, ShortcutGesture, Shortcuts};
use bo_music_app::likes::LikeState;
use gpui::{
    AnyElement, Context, Div, FocusHandle, Focusable, KeyDownEvent, Stateful, Window, div,
    prelude::*, px,
};

pub(super) struct ShortcutPage {
    pub bindings: Shortcuts,
    pub focus: FocusHandle,
    pub message: Option<String>,
    pub open: bool,
    recording: Option<ShortcutAction>,
}

impl ShortcutPage {
    pub(super) fn new(cx: &mut Context<MusicApp>) -> Self {
        Self {
            bindings: Shortcuts::default(),
            focus: cx.focus_handle(),
            message: None,
            open: false,
            recording: None,
        }
    }
}

impl MusicApp {
    pub(super) fn playback_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.auth.busy
            || self.show_account
            || self.playlist_dialog != PlaylistDialog::Closed
            || self.track_picker.is_some()
            || self.confirm_clear_search_history
            || [
                &self.input,
                &self.phone_input,
                &self.code_input,
                &self.comment_input,
                &self.playlist_name_input,
                &self.playlist_description_input,
                &self.preset_name_input,
                &self.preset_description_input,
            ]
            .iter()
            .any(|input| input.read(cx).focus_handle(cx).is_focused(window))
        {
            return;
        }
        let Some(action) = gesture(event).and_then(|key| self.shortcuts.bindings.action_for(&key))
        else {
            return;
        };
        if event.is_held
            && !matches!(
                action,
                ShortcutAction::VolumeUp | ShortcutAction::VolumeDown
            )
        {
            cx.stop_propagation();
            return;
        }
        let command = match action {
            ShortcutAction::PlayPause => Command::Toggle,
            ShortcutAction::Previous => Command::Previous,
            ShortcutAction::Next => Command::Next,
            ShortcutAction::VolumeUp => Command::VolumeDelta(5),
            ShortcutAction::VolumeDown => Command::VolumeDelta(-5),
            ShortcutAction::Mute => Command::ToggleMute,
            ShortcutAction::Favorite => {
                let Some(track) = self.player.as_ref().and_then(|player| player.track_id) else {
                    return;
                };
                if self.like.track_id != Some(track) {
                    return;
                }
                match self.like.state {
                    LikeState::Ready(_) => Command::ToggleLike(track),
                    LikeState::Unknown | LikeState::NoPlaylist => Command::RetryLike(track),
                    _ => return,
                }
            }
        };
        let _ = self.service.commands.send(command);
        cx.stop_propagation();
    }

    fn save_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.shortcuts.message = None;
        if self
            .service
            .commands
            .send(Command::SaveShortcuts(self.shortcuts.bindings.clone()))
            .is_err()
        {
            self.shortcuts.message = Some("无法保存快捷键".into());
        }
        cx.notify();
    }

    fn record_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.is_held {
            return;
        }
        if event.keystroke.key == "escape" {
            if self.shortcuts.recording.take().is_none() {
                self.close_shortcuts(window, cx);
            }
            self.shortcuts.message = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let Some(action) = self.shortcuts.recording else {
            return;
        };
        cx.stop_propagation();
        let Some(gesture) = gesture(event) else {
            if !matches!(event.keystroke.key.as_str(), "control" | "alt" | "shift") {
                self.shortcuts.message = Some("请按空格、功能键或带修饰键的组合键".into());
                cx.notify();
            }
            return;
        };
        match self.shortcuts.bindings.rebind(action, gesture) {
            Ok(()) => {
                self.shortcuts.recording = None;
                self.save_shortcuts(cx);
            }
            Err(conflict) => {
                self.shortcuts.message = Some(format!("该键位已用于「{}」", conflict.label()));
                cx.notify();
            }
        }
    }

    pub(super) fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcuts.open = true;
        self.shortcuts.recording = None;
        self.shortcuts.message = None;
        window.focus(&self.shortcuts.focus);
        cx.notify();
    }

    fn close_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcuts.open = false;
        self.appearance_page.open = true;
        self.shortcuts.recording = None;
        window.focus(&self.playback_focus);
        cx.notify();
    }

    pub(super) fn render_shortcuts(&self, cx: &mut Context<Self>) -> AnyElement {
        let rgb = crate::appearance::paint(cx);
        div()
            .id("shortcut-settings")
            .track_focus(&self.shortcuts.focus)
            .on_key_down(cx.listener(Self::record_shortcut))
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x20293a))
            .when_some(
                crate::appearance::font(crate::appearance::FontTarget::Interface, cx),
                |page, font| page.font_family(font),
            )
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        crate::ui::icon_button_labeled("shortcuts-back", "back", "返回设置", cx)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_shortcuts(window, cx)),
                            ),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("应用内快捷键"),
                    ),
            )
            .children(
                ShortcutAction::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, action)| {
                        let binding = if self.shortcuts.recording == Some(action) {
                            "请按键，Esc 取消".into()
                        } else {
                            self.shortcuts
                                .bindings
                                .binding(action)
                                .map_or_else(|| "未设置".into(), ShortcutGesture::label)
                        };
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .max_w(px(520.))
                            .rounded_lg()
                            .p_3()
                            .bg(rgb(0xffffff))
                            .child(action.label())
                            .child(button(("shortcut-record", index), binding, rgb).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.shortcuts.recording = Some(action);
                                    this.shortcuts.message = None;
                                    window.focus(&this.shortcuts.focus);
                                    cx.notify();
                                }),
                            ))
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        button("shortcuts-reset", "恢复默认", rgb).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.shortcuts.recording = None;
                                this.shortcuts.bindings = Shortcuts::default();
                                this.save_shortcuts(cx);
                            },
                        )),
                    )
                    .when_some(self.shortcuts.message.clone(), |row, message| {
                        row.child(message)
                    }),
            )
            .into_any_element()
    }
}

fn gesture(event: &KeyDownEvent) -> Option<ShortcutGesture> {
    let modifiers = event.keystroke.modifiers;
    if modifiers.platform || modifiers.function {
        return None;
    }
    ShortcutGesture::new(
        &event.keystroke.key,
        modifiers.control,
        modifiers.alt,
        modifiers.shift,
    )
}

fn button(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    rgb: impl Fn(u32) -> gpui::Rgba + Copy,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0xe8edff))
        .text_color(rgb(0x354db6))
        .hover(|button| button.bg(rgb(0xdbe4ff)))
        .child(text.into())
}
