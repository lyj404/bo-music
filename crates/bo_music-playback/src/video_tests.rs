use super::*;

#[test]
fn video_surface_discards_padding_and_fills_opaque_alpha() {
    let bytes = [1, 2, 3, 0, 9, 9, 9, 9, 4, 5, 6, 0, 8, 8, 8, 8];
    assert_eq!(to_bgra(&bytes, 8, 1), [1, 2, 3, 255, 4, 5, 6, 255]);
    assert_eq!(std::mem::size_of::<AlignedBlock>(), 64);
    assert_eq!(std::mem::align_of::<AlignedBlock>(), 64);
    assert!(VideoSize::new(1280, 720).is_ok());
    assert!(VideoSize::new(u32::MAX, 1).is_err());
    assert!(VideoSize::new(1280, 721).is_err());
}

#[cfg(windows)]
struct Clip(PathBuf);

#[cfg(windows)]
impl Clip {
    fn synthetic() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let file = std::env::temp_dir().join(format!(
            "bo_music-video-{}-{unique}.y4m",
            std::process::id()
        ));
        let mut bytes = b"YUV4MPEG2 W96 H54 F24:1 Ip A1:1 C420jpeg\n".to_vec();
        for index in 0..72 {
            bytes.extend_from_slice(b"FRAME\n");
            bytes.extend(std::iter::repeat_n(
                if index < 24 { 82 } else { 145 },
                96 * 54,
            ));
            bytes.extend(std::iter::repeat_n(90, 48 * 27));
            bytes.extend(std::iter::repeat_n(
                if index < 24 { 240 } else { 32 },
                48 * 27,
            ));
        }
        std::fs::write(&file, bytes).unwrap();
        Self(file)
    }

    fn open(&self, library: PathBuf) -> Result<VideoPlayer, VideoError> {
        VideoPlayer::open(
            library,
            LoadSource {
                address: self.0.to_string_lossy().into_owned(),
                start_seconds: None,
                end_seconds: None,
                initially_paused: true,
            },
            MpvOptions {
                audio_output: Some("null".into()),
            },
            VideoSize::new(97, 55).unwrap(),
        )
    }
}

#[cfg(windows)]
impl Drop for Clip {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn wait_until(player: &VideoPlayer, ready: impl Fn(&VideoSnapshot) -> bool) -> VideoSnapshot {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let snapshot = player.snapshot();
        if ready(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "video status: {:?}, failure: {:?}",
            snapshot.state,
            snapshot.failure
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
#[test]
fn bundled_audio_library_cannot_publish_video_frames() {
    let clip = Clip::synthetic();
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    match clip.open(library) {
        Ok(player) => {
            let view = wait_until(&player, |view| view.failure.is_some());
            assert!(view.frame.is_none());
            player.shutdown();
        }
        Err(VideoError::RendererUnavailable | VideoError::Mpv(MpvError::Native { .. })) => {}
        Err(error) => panic!("unexpected libmpv startup failure: {error}"),
    }
}

#[cfg(windows)]
#[test]
#[ignore = "requires the verified Windows full libmpv build in BO_MUSIC_TEST_VIDEO_LIBMPV"]
fn video_libmpv_renders_a_clip_and_updates_after_seek() {
    let clip = Clip::synthetic();
    let library = PathBuf::from(
        std::env::var_os("BO_MUSIC_TEST_VIDEO_LIBMPV").expect("missing video libmpv path"),
    );
    let player = clip.open(library).unwrap();
    let first = wait_until(&player, |view| {
        view.frame.is_some() && view.state == PlaybackState::Paused
    });
    let color = |frame: &VideoFrame| {
        let offset = ((frame.height / 2) * frame.width + frame.width / 2) as usize * 4;
        <[u8; 4]>::try_from(&frame.bgra[offset..offset + 4]).unwrap()
    };
    let original = color(first.frame.as_ref().unwrap());
    assert!(
        original[2] > original[1] && original[3] == 255,
        "first frame: {original:?}"
    );
    player.seek(1.5).unwrap();
    let after = wait_until(&player, |view| {
        view.frame
            .as_ref()
            .is_some_and(|frame| color(frame)[1] > color(frame)[2])
            && view.position_seconds >= 1.4
    });
    assert_eq!(after.state, PlaybackState::Paused);
    player.pause(false).unwrap();
    wait_until(&player, |view| view.state == PlaybackState::Playing);
    player.shutdown();
}
