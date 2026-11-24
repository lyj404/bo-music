use super::*;

impl MusicApp {
    pub(super) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.appearance_page.open = false;
        self.shortcuts.open = false;
        self.navigation.enter(page);
        self.row_actions = None;
        match page {
            Page::Home => {
                let _ = self.service.commands.send(Command::LoadHome);
            }
            Page::Library => {
                let _ = self.service.commands.send(Command::LoadLibrary);
            }
            Page::Charts => {
                let _ = self.service.commands.send(Command::Charts);
            }
            Page::Mine => {
                let _ = self.service.commands.send(Command::MyPlaylists);
            }
            Page::Collection => {
                let _ = self
                    .service
                    .commands
                    .send(Command::Collections(self.navigation.collection_kind));
            }
            Page::Recent => {
                let _ = self.service.commands.send(Command::ShowHistory);
            }
            Page::PlaylistSquare => {
                let _ = self.service.commands.send(Command::PlaylistSquare);
            }
            Page::AccountOverview => self.load_account_overview(cx),
            _ => self.run_search(cx),
        }
        cx.notify();
    }

    pub(super) fn can_go_back(&self) -> bool {
        !self.appearance_page.open && !self.shortcuts.open && self.navigation.can_go_back()
    }

    pub(super) fn go_back(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_back() {
            return;
        }
        self.row_actions = None;
        self.navigation.browse_description_open = false;
        let _ = self.service.commands.send(Command::BrowseBack);
        let _ = self.service.commands.send(Command::LoadDetails);
        cx.notify();
    }

    pub(super) fn render_navigation(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let mut nav = div()
            .id("main-navigation")
            .w(px(if compact { 174. } else { 204. }))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .px_3()
            .py_5()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .pb_6()
                    .child(ui::logo())
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("bo_music"),
                    ),
            );
        for (index, (page, icon, label)) in [
            (Page::Home, "home", "发现音乐"),
            (Page::Library, "grid", "音乐馆"),
            (Page::Charts, "chart", "排行榜"),
            (Page::PlaylistSquare, "music", "歌单广场"),
            (Page::Collection, "heart", "我的收藏"),
            (Page::Recent, "clock", "最近播放"),
            (Page::Mine, "list", "我的歌单"),
            (Page::AccountOverview, "chart", "听歌统计"),
        ]
        .into_iter()
        .enumerate()
        {
            if index == 4 {
                nav = nav.child(ui::section_label("我的音乐", cx));
            }
            let active =
                self.navigation.page == page && !self.appearance_page.open && !self.shortcuts.open;
            nav = nav.child(
                div()
                    .id(("navigation", index))
                    .relative()
                    .h(px(42.))
                    .my(px(2.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(rgb(0x687083))
                    .hover(|item| item.bg(rgb(0xe9edf5)))
                    .when(active, |item| {
                        item.bg(rgb(0xffffff))
                            .text_color(rgb(0x20293a))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .w(px(3.))
                                    .h(px(17.))
                                    .rounded_full()
                                    .bg(rgb(0x415de5)),
                            )
                    })
                    .child(ui::icon(
                        icon,
                        rgb(if active { 0x415de5 } else { 0x687083 }),
                    ))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.playback_focus);
                        this.navigate(page, cx);
                    })),
            );
        }
        nav.child(div().flex_1().min_h(px(24.)))
            .child(
                div()
                    .id("nav-settings")
                    .h(px(42.))
                    .px_3()
                    .rounded_md()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(self.appearance_page.open || self.shortcuts.open, |item| {
                        item.bg(rgb(0xffffff))
                    })
                    .hover(|item| item.bg(rgb(0xe9edf5)))
                    .child(ui::icon("settings", rgb(0x687083)))
                    .child("设置")
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.playback_focus);
                        this.appearance_page.open = true;
                        this.shortcuts.open = false;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
