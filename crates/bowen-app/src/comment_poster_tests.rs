use super::*;

#[test]
fn renders_long_comment_and_song_code_to_a_bounded_png() {
    let comment = SongComment {
        id: 1,
        user_id: 0,
        parent_id: 0,
        reply_id: 0,
        reply_nickname: String::new(),
        nickname: "A&B <作者>".into(),
        avatar_url: None,
        content: "长评论 <A&B>\n".repeat(40),
        image_url: None,
        publish_time: String::new(),
        location: String::new(),
        like_count: 2,
        reply_count: 0,
        is_liked: false,
    };
    let track = Track {
        id: 5,
        title: "歌曲".into(),
        artist_text: "歌手".into(),
        artists: vec![],
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 0,
        available_qualities: vec![],
        audio_variants: vec![],
        requires_vip: false,
        requires_purchase: false,
        lyrics: None,
        has_mv: false,
        comment_count: None,
        favorite_count: None,
        share_count: None,
    };
    let lines = wrap(&comment.content);
    assert_eq!(lines.len(), 18);
    assert!(lines[17].ends_with('…'));
    let png = make_poster(
        &comment,
        &track,
        Some("https://example.test/song/5"),
        None,
        None,
        None,
    )
    .unwrap();
    let pixels = image::load_from_memory(&png).unwrap().into_rgb8();
    assert_eq!(pixels.dimensions(), (760, 1122));
    assert!((600..700).any(|x| pixels.get_pixel(x, 980).0 == [255, 255, 255]));
}
