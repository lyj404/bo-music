use super::*;

impl MusicApp {
    pub(super) fn render_shell_header(
        &self,
        show_search_assist: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let account_open = self.show_account;
        let history_open = self.show_search_history && !self.search_assist_dismissed;
        let hot_open = self.show_hot_words && !self.search_assist_dismissed;
        let account_label = self
            .auth
            .account
            .as_ref()
            .and_then(|account| account.nickname.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "登录".into());
        div()
            .h(px(72.))
            .flex_shrink_0()
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .child(
                ui::icon_button("header-back", "back", cx).on_click(cx.listener(
                    |this, _, _, cx| {
                        if this.browse.as_ref().is_some_and(|view| view.is_detail) {
                            this.home_playlist_open = false;
                            this.library_album_open = false;
                            this.overview_detail_open = false;
                            let _ = this.service.commands.send(Command::BrowseBack);
                            let _ = this.service.commands.send(Command::LoadDetails);
                        } else {
                            this.navigate(Page::Home, cx);
                        }
                    },
                )),
            )
            .child(
                div()
                    .relative()
                    .max_w(px(340.))
                    .flex_1()
                    .min_w(px(160.))
                    .rounded_md()
                    .bg(rgb(0xf5f7fb))
                    .border_1()
                    .border_color(rgb(0xd8dce6))
                    .flex()
                    .items_center()
                    .px_3()
                    .gap_2()
                    .when(show_search_assist, |search| {
                        search.child(gpui::deferred(self.render_search_assist(cx)).with_priority(1))
                    })
                    .child(ui::icon("search", rgb(0x788196)))
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(0x788196))
                            .child("↵"),
                    ),
            )
            .child(
                ui::icon_button("header-search", "search", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.run_search(cx))),
            )
            .child(div().flex_1())
            .child(
                ui::icon_button("search-hot-words", "chart", cx).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        if this.auth.busy || this.show_account {
                            return;
                        }
                        if hot_open {
                            this.dismiss_search_assist(cx);
                        } else {
                            this.search_assist_dismissed = false;
                            this.show_hot_words = true;
                            this.show_search_history = false;
                            this.confirm_clear_search_history = false;
                            if this.hot_words.is_none() {
                                this.load_hot_words(cx);
                            }
                            cx.notify();
                        }
                    }),
                ),
            )
            .child(
                ui::icon_button("search-history-toggle", "clock", cx).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        if this.auth.busy || this.show_account {
                            return;
                        }
                        if history_open {
                            this.dismiss_search_assist(cx);
                        } else {
                            this.search_assist_dismissed = false;
                            this.show_search_history = true;
                            this.show_hot_words = false;
                            this.request_search_history(Command::LoadSearchHistory, cx);
                            cx.notify();
                        }
                    }),
                ),
            )
            .child(
                ui::icon_button("header-theme", "sun", cx).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.toggle_theme(cx);
                    },
                )),
            )
            .child(
                div()
                    .id("account")
                    .max_w(px(130.))
                    .px_3()
                    .py_2()
                    .rounded_full()
                    .bg(rgb(0xe9edff))
                    .text_color(rgb(0x415de5))
                    .text_sm()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .child(div().min_w_0().truncate().child(account_label))
                    .child(ui::icon("collapse", rgb(0x415de5)).size(px(14.)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if account_open {
                                this.close_account_menu(cx);
                            } else {
                                this.show_account = true;
                                this.player_tools_open = false;
                                let _ = this.service.commands.send(Command::RefreshAccounts);
                                cx.notify();
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn close_account_menu(&mut self, cx: &mut Context<Self>) {
        if self.show_account && self.show_qr && !self.auth.busy {
            let _ = self.service.commands.send(Command::CancelQr);
        }
        self.show_account = false;
        self.show_qr = false;
        self.forget_pending = None;
        self.adding_account = false;
        cx.notify();
    }

    pub(super) fn render_account_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        if let Some(account) = &self.auth.account
            && !self.adding_account
        {
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_start()
                .gap_4()
                .child(format!(
                    "当前账号：{}",
                    account
                        .nickname
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&account.uid)
                ))
                .child(
                    div()
                        .id("logout")
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0xe9edf5))
                        .child("退出登录")
                        .when(!self.auth.busy, |button| {
                            button
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, _| {
                                    let _ = this.service.commands.send(Command::SignOut);
                                }))
                        }),
                )
                .child(
                    div()
                        .id("account-add")
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0xe9edf5))
                        .child("添加账号")
                        .when(!self.auth.busy, |button| {
                            button
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.adding_account = true;
                                    cx.notify();
                                }))
                        }),
                )
                .into_any_element()
        } else if self.show_qr {
            let qr_image = match &self.qr {
                QrDisplay::Ready { width, modules } => {
                    let cell = if *width <= 45 {
                        4.0
                    } else if *width <= 65 {
                        3.0
                    } else if *width <= 90 {
                        2.0
                    } else {
                        1.0
                    };
                    let rows = modules
                        .chunks(*width)
                        .map(|row| {
                            div().flex().children(row.iter().map(|dark| {
                                div().size(px(cell)).bg(gpui::rgb(if *dark {
                                    0x000000
                                } else {
                                    0xffffff
                                }))
                            }))
                        })
                        .collect::<Vec<_>>();
                    div()
                        .bg(gpui::rgb(0xffffff))
                        .p(px(cell * 4.0))
                        .flex()
                        .flex_col()
                        .children(rows)
                        .into_any_element()
                }
                QrDisplay::Loading => div()
                    .w(px(180.))
                    .child("正在获取二维码…")
                    .into_any_element(),
                QrDisplay::Expired => div().w(px(180.)).child("二维码已过期").into_any_element(),
                QrDisplay::Failed => div()
                    .w(px(180.))
                    .child("二维码获取或登录失败")
                    .into_any_element(),
                QrDisplay::Idle => div().w(px(180.)).child("点击获取二维码").into_any_element(),
            };
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_start()
                .gap_4()
                .child(qr_image)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child("使用波点 App 扫描二维码")
                        .child(
                            div()
                                .id("qr-refresh")
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(rgb(0xe9edf5))
                                .child("刷新二维码")
                                .when(!self.auth.busy, |button| {
                                    button.cursor_pointer().on_click(cx.listener(
                                        |this, _, _, _| {
                                            let _ = this.service.commands.send(Command::StartQr);
                                        },
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .id("phone-tab")
                                .px_3()
                                .py_2()
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("手机号登录")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.auth.busy {
                                        let _ = this.service.commands.send(Command::CancelQr);
                                        this.show_qr = false;
                                        cx.notify();
                                    }
                                })),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_start()
                .gap_3()
                .child(div().w_full().child(self.phone_input.clone()))
                .child(
                    div()
                        .id("send-sms")
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0xe9edf5))
                        .child("发送验证码")
                        .when(!self.auth.busy, |button| {
                            button
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let mobile = this.phone_input.read(cx).text().to_owned();
                                    let _ = this.service.commands.send(Command::RequestSms(mobile));
                                }))
                        }),
                )
                .child(div().w_full().child(self.code_input.clone()))
                .child(
                    div()
                        .id("phone-login")
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0x4d66e5))
                        .text_color(gpui::rgb(0xffffff))
                        .child("登录")
                        .when(!self.auth.busy, |button| {
                            button
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let mobile = this.phone_input.read(cx).text().to_owned();
                                    let code = this.code_input.read(cx).text().to_owned();
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::LoginPhone(mobile, code));
                                }))
                        }),
                )
                .child(
                    div()
                        .id("qr-tab")
                        .px_3()
                        .py_2()
                        .text_color(rgb(0x415de5))
                        .child("扫码登录")
                        .when(!self.auth.busy, |button| {
                            button
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_qr = true;
                                    let _ = this.service.commands.send(Command::StartQr);
                                    cx.notify();
                                }))
                        }),
                )
                .into_any_element()
        }
    }
}
