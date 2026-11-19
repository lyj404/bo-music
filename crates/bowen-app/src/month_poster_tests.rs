use super::*;
use bowen_domain::account::{MonthPeriod, ReportTrack};

#[test]
fn posters_render_escaped_text_and_embedded_artwork_to_png() {
    let report = MonthReport {
        period: MonthPeriod::new(2026, 10).unwrap(),
        listen_total_minutes: Some("128".into()),
        play_count: Some(42),
        song_count: Some(12),
        time_rank: Some(7),
        listen_chain: None,
        play_chain: None,
        listen_content: None,
        listen_top_track_title: None,
        play_content: None,
        artist_name: Some("歌手".into()),
        artist_cover_url: None,
        artist_music_name: None,
        artist_music_cover_url: None,
        artist_content: None,
        top_tracks: vec![ReportTrack {
            id: 1,
            title: "歌曲 <A&B>".into(),
            artist: "歌手".into(),
            album_name: None,
            cover_url: None,
            play_count: Some(8),
        }],
        album_covers: Vec::new(),
    };
    let mut cover = tiny_skia::Pixmap::new(4, 4).unwrap();
    cover.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
    let covers = [Some(cover.encode_png().unwrap())];
    for (kind, dark, cover_sample, background) in [
        (MonthPosterKind::Overview, true, (100, 800), [21, 25, 50]),
        (MonthPosterKind::Tracks, false, (100, 320), [241, 243, 255]),
    ] {
        let svg = svg(
            &report,
            Some("A&B <C>"),
            MonthPosterStyle { kind, dark },
            &covers,
        );
        assert!(svg.contains("A&amp;B &lt;C&gt;"));
        let png = rasterize(&svg).unwrap();
        let pixels = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .into_rgb8();
        assert_eq!(pixels.dimensions(), (WIDTH, HEIGHT));
        assert_eq!(pixels.get_pixel(0, 0).0, background);
        assert_eq!(
            pixels.get_pixel(cover_sample.0, cover_sample.1).0,
            [255, 0, 0]
        );
    }
}
