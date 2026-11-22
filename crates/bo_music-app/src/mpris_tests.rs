use super::*;

fn paused_track() -> Snapshot {
    Snapshot {
        state: PlaybackState::Paused,
        track: Some(Track {
            account: 2,
            id: 17,
            title: "测试".into(),
            artist: "歌手".into(),
            album: Some("专辑".into()),
            duration: 3_000_000,
            cover: Some("https://example.invalid/cover.png".into()),
        }),
        position: 1_234_000,
        ..Snapshot::default()
    }
}

#[test]
fn mpris_rejects_stale_and_out_of_range_absolute_positions() {
    let state = paused_track();
    let current = state.track.as_ref().unwrap().path();
    assert!(matches!(
        state.absolute_seek(&current, 1_250_000),
        Some(Command::SeekTo(2, 17, 1.25))
    ));
    assert!(state.absolute_seek(&current, -1).is_none());
    assert!(state.absolute_seek(&current, 3_000_001).is_none());
    assert!(
        state
            .absolute_seek(&ObjectPath::try_from("/old").unwrap(), 0)
            .is_none()
    );
    assert!(matches!(
        state.relative_seek(-500_000),
        Some(Command::SeekFromMedia(2, 17, -500_000))
    ));
}

#[test]
fn mpris_metadata_and_property_changes_exclude_the_polling_position() {
    let before = Snapshot::default();
    let after = paused_track();
    let changes = changed_properties(&before, &after);
    assert_eq!(
        <&str>::try_from(&changes["PlaybackStatus"]).unwrap(),
        "Paused"
    );
    let metadata: HashMap<String, OwnedValue> =
        changes["Metadata"].try_clone().unwrap().try_into().unwrap();
    assert_eq!(i64::try_from(&metadata["mpris:length"]).unwrap(), 3_000_000);
    assert_eq!(<&str>::try_from(&metadata["xesam:album"]).unwrap(), "专辑");
    assert_eq!(
        Vec::<String>::try_from(metadata["xesam:artist"].try_clone().unwrap()).unwrap(),
        ["歌手"]
    );
    let moved = Snapshot {
        position: 2_180_000,
        ..after.clone()
    };
    assert_eq!(moved.position(), 2_180_000);
    assert!(changed_properties(&after, &moved).is_empty());
}

#[test]
fn local_artwork_is_published_and_removed_when_the_account_changes() {
    let before = paused_track();
    let mut media = MediaArtwork::default();
    media.select(before.track.as_ref(), None);
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let cover = CoverFile::write(png.get_ref()).unwrap();
    let path = reqwest::Url::parse(&cover.url)
        .unwrap()
        .to_file_path()
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), *png.get_ref());
    let after = Snapshot {
        art_url: Some(cover.url.clone()),
        ..before.clone()
    };
    assert_eq!(
        <&str>::try_from(&after.metadata()["mpris:artUrl"]).unwrap(),
        cover.url
    );
    assert!(changed_properties(&before, &after).contains_key("Metadata"));
    media.ready = Some(cover);
    media.select(before.track.as_ref(), None);
    assert!(path.exists());
    let mut next = before.track.clone().unwrap();
    next.account = 3;
    media.select(Some(&next), None);
    assert!(media.ready.is_none());
    assert!(!path.exists());
}
