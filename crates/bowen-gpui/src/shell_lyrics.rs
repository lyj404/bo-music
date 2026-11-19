use super::*;

impl MusicApp {
    pub(super) fn render_lyrics_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        match &self.lyrics {
            LyricsView::Empty => div()
                .p_3()
                .text_color(rgb(0x8a91a2))
                .child("暂无歌词")
                .into_any_element(),
            LyricsView::Loading => div()
                .p_3()
                .text_color(rgb(0x8a91a2))
                .child("正在加载歌词…")
                .into_any_element(),
            LyricsView::Failed => div()
                .p_3()
                .text_color(rgb(0x8a91a2))
                .child("歌词加载失败")
                .into_any_element(),
            LyricsView::Ready(document) => {
                let position = self.player.as_ref().map_or(0, |player| player.position_ms);
                let stamp = self.lyric_seek_stamp();
                let active = document
                    .line_at(position)
                    .and_then(|index| document.source_line(index));
                let units = document.scroll_units();
                let active_unit = active.and_then(|index| units.get(index).copied());
                let lines = document
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| {
                        let current =
                            active_unit.is_some_and(|unit| units.get(index) == Some(&unit));
                        let target = line.start_ms as f64 / 1000.0;
                        let mut content = div().flex().flex_wrap();
                        if current
                            && !line.is_translation
                            && document.kind == bowen_domain::lyrics::LyricKind::WordByWord
                        {
                            for syllable in &line.syllables {
                                content = content.child(
                                    div()
                                        .text_color(if position >= syllable.start_ms {
                                            rgb(0x415de5)
                                        } else {
                                            rgb(0x9ca4b4)
                                        })
                                        .child(syllable.text.clone()),
                                );
                            }
                        } else {
                            content = content.child(line.text.clone());
                        }
                        div()
                            .id(("lyric", index))
                            .p_2()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(if current {
                                rgb(0x415de5)
                            } else {
                                rgb(0x8a91a2)
                            })
                            .when(line.is_translation, |row| row.text_sm())
                            .hover(|row| row.bg(rgb(0xe9edf5)))
                            .on_click(cx.listener(move |this, _, _, _| {
                                this.seek_lyric(stamp, target);
                            }))
                            .child(content)
                    })
                    .collect::<Vec<_>>();
                div()
                    .id("lyrics")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.lyric_scroll)
                    .on_scroll_wheel(cx.listener(|this, _, _, _| {
                        this.manual_scroll_until = Some(Instant::now() + Duration::from_secs(5));
                    }))
                    .children(lines)
                    .into_any_element()
            }
        }
    }
}
