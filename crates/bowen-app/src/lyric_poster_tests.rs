use super::*;

#[test]
fn lyric_poster_limits_rows_and_renders_each_background() {
    let mut input = Lyrics {
        title: "夜曲 < A & B".into(),
        artist: "歌手".into(),
        lines: vec![" ".into()],
        style: LyricPosterStyle::default(),
        cover_rgb: Some(0x834c72),
    };
    assert!(render(&input).is_err());
    input
        .lines
        .extend(std::iter::repeat_n("逐字歌词 & 标点".into(), MAX_LINES + 4));
    let dark = render(&input).unwrap();
    input.style.theme = PosterTheme::Warm;
    let warm = render(&input).unwrap();
    assert_ne!(dark, warm);
    let image = image::load_from_memory(&warm).unwrap();
    assert_eq!(
        (image.width(), image.height()),
        (760, 230 + MAX_LINES as u32 * 56)
    );
    input.style.background_rgb = Some(0x103264);
    input.style.font_family = "missing test font".into();
    let custom = image::load_from_memory(&render(&input).unwrap())
        .unwrap()
        .into_rgba8();
    assert_eq!(custom.get_pixel(0, 0).0, [0x10, 0x32, 0x64, 255]);
    assert_eq!(custom.get_pixel(60, 47).0, [255; 4]);
}
