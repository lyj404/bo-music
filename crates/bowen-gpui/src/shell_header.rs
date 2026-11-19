use super::*;

impl MusicApp {
    pub(super) fn render_shell_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let account_label = self
            .auth
            .account
            .as_ref()
            .map(|account| {
                account
                    .nickname
                    .clone()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("账号 {}", account.uid))
            })
            .unwrap_or_else(|| "登录".into());

        div()
            .h(px(70.))
            .px_6()
            .flex()
            .items_center()
            .gap_5()
            .bg(rgb(0xffffff))
            .child(
                div()
                    .text_xl()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(0x415de5))
                    .child("波纹音乐"),
            )
            .child(div().flex_1().max_w(px(480.)).child(self.input.clone()))
            .child(
                div()
                    .id("open-appearance")
                    .cursor_pointer()
                    .text_color(rgb(0x415de5))
                    .child("外观")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.appearance_page.open = true;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("open-shortcut-settings")
                    .cursor_pointer()
                    .text_color(rgb(0x415de5))
                    .child("快捷键")
                    .on_click(cx.listener(|this, _, window, cx| this.open_shortcuts(window, cx))),
            )
            .child(
                div()
                    .id("search-hot-words")
                    .cursor_pointer()
                    .text_color(rgb(0x415de5))
                    .child("热搜")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.auth.busy || this.show_account {
                            return;
                        }
                        this.show_hot_words = !this.show_hot_words;
                        this.show_search_history = false;
                        this.confirm_clear_search_history = false;
                        if this.show_hot_words && this.hot_words.is_none() {
                            this.load_hot_words(cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("search-history-toggle")
                    .cursor_pointer()
                    .text_color(rgb(0x415de5))
                    .child("历史")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.auth.busy || this.show_account {
                            return;
                        }
                        this.show_search_history = !this.show_search_history;
                        this.show_hot_words = false;
                        this.confirm_clear_search_history = false;
                        if this.show_search_history {
                            this.request_search_history(Command::LoadSearchHistory, cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("search")
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x4d66e5))
                    .text_color(gpui::rgb(0xffffff))
                    .cursor_pointer()
                    .child("搜索")
                    .hover(|style| style.bg(rgb(0x3854d5)))
                    .on_click(cx.listener(|this, _, _, cx| this.run_search(cx))),
            )
            .child(
                div()
                    .id("account")
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(rgb(0x415de5))
                    .child(account_label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.show_account && this.show_qr && !this.auth.busy {
                            let _ = this.service.commands.send(Command::CancelQr);
                            this.show_qr = false;
                        }
                        this.show_account = !this.show_account;
                        if !this.show_account {
                            this.forget_pending = None;
                            this.adding_account = false;
                        }
                        if this.show_account {
                            let _ = this.service.commands.send(Command::RefreshAccounts);
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(super) fn render_account_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        if let Some(account) = &self.auth.account
            && !self.adding_account
        {
            div()
                .flex()
                .items_center()
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
                .flex()
                .items_center()
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
                .flex()
                .items_center()
                .gap_3()
                .child(div().w(px(200.)).child(self.phone_input.clone()))
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
                .child(div().w(px(150.)).child(self.code_input.clone()))
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
