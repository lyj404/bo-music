use super::*;

impl MusicApp {
    pub(super) fn render_queue_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let queue = self
            .player
            .as_ref()
            .map(|p| p.queue.clone())
            .unwrap_or_default();
        let queue_len = queue.len();
        let selected = self.player.as_ref().and_then(|p| p.selected);
        let queue_stamp = self
            .player
            .as_ref()
            .map(|p| (p.queue_account_revision, p.queue_revision));
        let queue_reorderable = self.player.as_ref().is_some_and(|p| p.queue_reorderable);
        let (account_revision, queue_revision) = queue_stamp.unwrap_or((0, 0));
        let confirming_clear = self.clear_queue_pending == queue_stamp && queue_stamp.is_some();
        div()
            .id("queue")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .when(queue.is_empty(), |view| {
                view.child(div().text_color(rgb(0x8a91a2)).child("队列为空"))
            })
            .when(!queue.is_empty(), |view| {
                view.child(
                    div()
                        .id("queue-clear")
                        .p_2()
                        .text_sm()
                        .cursor_pointer()
                        .text_color(rgb(0xc23b46))
                        .child(if confirming_clear {
                            "再次点击确认清空队列"
                        } else {
                            "清空队列"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let stamp = (account_revision, queue_revision);
                            if this.clear_queue_pending == Some(stamp) {
                                this.clear_queue_pending = None;
                                let _ = this.service.commands.send(Command::EditQueue(
                                    account_revision,
                                    queue_revision,
                                    QueueEdit::Clear,
                                ));
                            } else {
                                this.clear_queue_pending = Some(stamp);
                            }
                            cx.notify();
                        })),
                )
            })
            .children(queue.into_iter().enumerate().map(|(index, item)| {
                div()
                    .id(("queued", index))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected == Some(index), |view| {
                        view.bg(rgb(0xe9edff)).text_color(rgb(0x415de5))
                    })
                    .hover(|view| view.bg(rgb(0xe9edf5)))
                    .on_click(cx.listener(move |this, _, _, _| {
                        let _ = this.service.commands.send(Command::EditQueue(
                            account_revision,
                            queue_revision,
                            QueueEdit::Select(index),
                        ));
                    }))
                    .child(div().flex_1().overflow_hidden().child(item))
                    .when(queue_reorderable && index > 0, |row| {
                        row.child(div().id(("queue-up", index)).text_sm().child("↑").on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                let _ = this.service.commands.send(Command::EditQueue(
                                    account_revision,
                                    queue_revision,
                                    QueueEdit::Move(index, index - 1),
                                ));
                            }),
                        ))
                    })
                    .when(queue_reorderable && index + 1 < queue_len, |row| {
                        row.child(
                            div()
                                .id(("queue-down", index))
                                .text_sm()
                                .child("↓")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let _ = this.service.commands.send(Command::EditQueue(
                                        account_revision,
                                        queue_revision,
                                        QueueEdit::Move(index, index + 1),
                                    ));
                                })),
                        )
                    })
                    .child(
                        div()
                            .id(("queue-remove", index))
                            .text_sm()
                            .text_color(rgb(0xc23b46))
                            .child("移除")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                let _ = this.service.commands.send(Command::EditQueue(
                                    account_revision,
                                    queue_revision,
                                    QueueEdit::Remove(index),
                                ));
                            })),
                    )
            }))
            .into_any_element()
    }
}
