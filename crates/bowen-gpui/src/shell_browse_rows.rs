use super::*;

impl MusicApp {
    pub(super) fn render_browse_rows(
        &self,
        view: &BrowseView,
        selected: &[(usize, i64)],
        queue_stamp: Option<(u64, u64)>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let rgb = appearance::paint(cx);
        match &view.content {
            BrowseContent::Charts(sections) => sections
                .iter()
                .enumerate()
                .map(|(section_index, section)| {
                    let rows = section
                        .charts
                        .iter()
                        .enumerate()
                        .map(|(index, chart)| {
                            let chart = chart.clone();
                            let preview = chart
                                .preview_tracks
                                .iter()
                                .take(3)
                                .map(|track| track.title.as_str())
                                .collect::<Vec<_>>()
                                .join(" · ");
                            div()
                                .id(("chart", section_index * 10000 + index))
                                .flex()
                                .items_center()
                                .gap_3()
                                .p_3()
                                .rounded_md()
                                .hover(|style| style.bg(rgb(0xe9edf5)).cursor_pointer())
                                .child(self.cover(chart.cover_url.as_deref(), 48.0, cx))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(div().child(chart.name.clone()))
                                        .child(
                                            div().text_sm().text_color(rgb(0x8a91a2)).child(
                                                format!("{}  {}", chart.update_text, preview),
                                            ),
                                        ),
                                )
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::OpenChart(chart.clone()));
                                }))
                        })
                        .collect::<Vec<_>>();
                    div()
                        .child(
                            div()
                                .p_3()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(section.title.clone()),
                        )
                        .children(rows)
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
            BrowseContent::Playlists(items) => items
                .iter()
                .enumerate()
                .map(|(index, playlist)| {
                    let playlist = playlist.clone();
                    div()
                        .id(("playlist", index))
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_3()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(0xe9edf5)))
                        .child(self.cover(playlist.cover_url.as_deref(), 48.0, cx))
                        .child(div().flex_1().child(playlist.name.clone()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0x8a91a2))
                                .child(format!("{} 首", playlist.music_count)),
                        )
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::OpenPlaylist(playlist.clone()));
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
            BrowseContent::Albums(items) => items
                .iter()
                .enumerate()
                .map(|(index, album)| {
                    let album = album.clone();
                    div()
                        .id(("album", index))
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_3()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(0xe9edf5)))
                        .child(self.cover(album.cover_url.as_deref(), 48.0, cx))
                        .child(div().flex_1().child(album.name.clone()))
                        .child(
                            div()
                                .w(px(150.))
                                .text_color(rgb(0x687083))
                                .child(album.artist_text.clone()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0x8a91a2))
                                .child(format!("{} 首", album.music_count)),
                        )
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::OpenAlbum(album.clone()));
                            let _ = this.service.commands.send(Command::LoadDetails);
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
            BrowseContent::Artists(items) => items
                .iter()
                .enumerate()
                .map(|(index, artist)| {
                    let artist = artist.clone();
                    div()
                        .id(("artist", index))
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_3()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(0xe9edf5)))
                        .child(self.cover(artist.cover_url.as_deref(), 48.0, cx))
                        .child(div().flex_1().child(artist.name.clone()))
                        .child(div().text_sm().text_color(rgb(0x8a91a2)).child(format!(
                            "{} 首 · {} 张专辑",
                            artist.song_count, artist.album_count
                        )))
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::OpenArtist(artist.clone()));
                            let _ = this.service.commands.send(Command::LoadDetails);
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
            BrowseContent::Tracks(tracks) => tracks
                .iter()
                .enumerate()
                .map(|(index, track)| {
                    let track_id = track.id;
                    let picker_track = track.clone();
                    let generation = view.generation;
                    let editable_playlist = view.editable_playlist;
                    let checked = selected.contains(&(index, track_id));
                    div()
                        .id(("playlist-track", index))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_3()
                        .p_3()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(0xe9edf5)))
                        .when(track_id > 0, |row| {
                            row.child(
                                div()
                                    .id(("batch-select-track", index))
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child(if checked { "☑" } else { "☐" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if this.batch_generation
                                            != Some(BatchSource::Browse(generation))
                                        {
                                            this.batch_selection.clear();
                                            this.batch_generation =
                                                Some(BatchSource::Browse(generation));
                                        }
                                        if !this.batch_selection.insert((index, track_id)) {
                                            this.batch_selection.remove(&(index, track_id));
                                        }
                                        this.batch_remove_confirm = false;
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(self.cover(track.cover_url.as_deref(), 40.0, cx))
                        .child(
                            div()
                                .w(px(36.))
                                .text_color(rgb(0x8a91a2))
                                .child(format!("{:02}", index + 1)),
                        )
                        .child(div().flex_1().min_w(px(120.)).child(track.title.clone()))
                        .child(
                            div()
                                .w(px(100.))
                                .text_color(rgb(0x687083))
                                .child(track.artist_text.clone()),
                        )
                        .child(
                            div()
                                .id(("browse-track-add", index))
                                .text_sm()
                                .text_color(rgb(0x415de5))
                                .child("加入歌单")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::OpenTrackPicker(picker_track.clone()));
                                })),
                        )
                        .when(queue_stamp.is_some() && track_id > 0, |row| {
                            let (account, revision) = queue_stamp.expect("queue checked");
                            row.child(
                                div()
                                    .id(("browse-track-append", index))
                                    .text_sm()
                                    .text_color(rgb(0x415de5))
                                    .child("排队")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        let _ =
                                            this.service.commands.send(Command::EditBrowseQueue(
                                                account, revision, index, track_id, generation,
                                                false,
                                            ));
                                    })),
                            )
                            .child(
                                div()
                                    .id(("browse-track-next", index))
                                    .text_sm()
                                    .text_color(rgb(0x415de5))
                                    .child("下一首")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        let _ =
                                            this.service.commands.send(Command::EditBrowseQueue(
                                                account, revision, index, track_id, generation,
                                                true,
                                            ));
                                    })),
                            )
                        })
                        .when(editable_playlist.is_some() && !self.playlist_busy, |row| {
                            let playlist_id = editable_playlist.expect("playlist checked");
                            row.child(
                                div()
                                    .id(("browse-track-remove", index))
                                    .text_sm()
                                    .text_color(rgb(0xc23b46))
                                    .child("移除")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        let _ = this.service.commands.send(
                                            Command::RemovePlaylistTrack(
                                                playlist_id,
                                                index,
                                                track_id,
                                                generation,
                                            ),
                                        );
                                    })),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::PlayBrowse(index, track_id, generation));
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
        }
    }
}
