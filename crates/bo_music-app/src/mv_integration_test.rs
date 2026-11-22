use super::runtime::ServiceOptions;
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[track_caller]
fn wait(service: &Service, accepts: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut status = String::new();
    loop {
        assert!(
            Instant::now() < deadline,
            "expected MV service event; last status: {status}"
        );
        if let Ok(event) = service.events.recv_timeout(Duration::from_millis(150)) {
            if accepts(&event) {
                return event;
            }
            if let Event::Status(message) = event {
                status = message;
            }
        }
    }
}

fn silent_wav() -> Vec<u8> {
    const AUDIO_BYTES: u32 = 8_000 * 2 * 24;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + AUDIO_BYTES).to_le_bytes());
    bytes.extend(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    bytes.extend(8_000_u32.to_le_bytes());
    bytes.extend(16_000_u32.to_le_bytes());
    bytes.extend(b"\x02\0\x10\0data");
    bytes.extend(AUDIO_BYTES.to_le_bytes());
    bytes.resize(bytes.len() + AUDIO_BYTES as usize, 0);
    bytes
}

#[test]
#[ignore = "requires the verified Windows full libmpv build in BO_MUSIC_TEST_VIDEO_LIBMPV"]
fn mv_service_keeps_quality_when_replacement_is_unavailable_and_resumes_audio() {
    let library = PathBuf::from(std::env::var_os("BO_MUSIC_TEST_VIDEO_LIBMPV").unwrap());
    let audio_library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let root = std::env::temp_dir().join(format!(
        "bo_music-mv-service-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = DataPaths::new(&root);
    let track = Track {
        id: 7,
        title: "测试 MV".into(),
        artist_text: String::new(),
        artists: vec![],
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 6,
        available_qualities: vec![AudioQuality::Standard],
        audio_variants: vec![],
        requires_vip: false,
        requires_purchase: false,
        lyrics: None,
        has_mv: true,
        comment_count: None,
        favorite_count: None,
        share_count: None,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let alive = Arc::clone(&running);
    listener.set_nonblocking(true).unwrap();
    let mock = std::thread::spawn(move || {
        let audio = silent_wav();
        let playback = format!(
            r#"{{"code":200,"data":{{"audioUrl":"http://{address}/fixture.wav","format":"mp3","bitrate":128}}}}"#
        );
        let mut clip = b"YUV4MPEG2 W32 H18 F12:1 Ip A1:1 C420jpeg\n".to_vec();
        for _ in 0..72 {
            clip.extend(b"FRAME\n");
            clip.extend(std::iter::repeat_n(128, 32 * 18 * 3 / 2));
        }
        while alive.load(Ordering::Acquire) {
            let Ok((mut socket, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut request = String::new();
            if reader
                .read_line(&mut request)
                .ok()
                .filter(|size| *size > 0)
                .is_none()
            {
                continue;
            }
            let mut length = 0;
            for line in reader
                .by_ref()
                .lines()
                .map_while(Result::ok)
                .take_while(|line| !line.is_empty())
            {
                if let Some((key, value)) = line.split_once(':')
                    && key.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            if reader.read_exact(&mut vec![0; length]).is_err() {
                continue;
            }
            let success = format!(
                r#"{{"code":200,"data":{{"mv":{{"highUrl":"http://{address}/fixture.y4m","playLimitTime":4}}}}}}"#
            );
            let body = if request.starts_with("GET /fixture.y4m") {
                clip.as_slice()
            } else if request.starts_with("GET /fixture.wav") {
                audio.as_slice()
            } else if request.starts_with("GET /api/play/music/v2/checkRight?") {
                br#"{"code":200,"data":{"status":4}}"#
            } else if request.starts_with("GET /api/play/music/v2/audioUrl?") {
                playback.as_bytes()
            } else if request.starts_with("GET /api/service/mv/info?") && request.contains("wifi=0")
            {
                br#"{"code":20048}"#
            } else if request.starts_with("GET /api/service/mv/info?") {
                success.as_bytes()
            } else {
                br#"{"code":200,"data":{}}"#
            };
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(body);
        }
    });
    let service = Service::start_with(ServiceOptions {
        paths: Some(paths),
        base_url: Some(format!("http://{address}/api/")),
        lyrics_endpoint: Some(format!("http://{address}/lyrics")),
        library: Some(audio_library),
        video_library: Some(library),
        audio_output: Some("null".into()),
        ..Default::default()
    });
    let command = |command| service.commands.send(command).unwrap();
    command(Command::PlaySingle(track));
    let Event::Player(audio) = wait(
        &service,
        |event| matches!(event, Event::Player(audio) if audio.track_id == Some(7) && audio.state == PlaybackState::Playing),
    ) else {
        unreachable!()
    };
    let completed = || {
        let Event::Mv(Some(view)) = wait(
            &service,
            |event| matches!(event, Event::Mv(Some(mv)) if mv.requested.is_none()),
        ) else {
            unreachable!()
        };
        view
    };
    command(Command::OpenMv(audio.queue_account_revision, 7));
    let high = completed();
    assert!(high.error.is_none(), "{:?}", high.error);
    assert_eq!(high.quality, Some(MvQuality::High));
    assert_eq!(high.preview_seconds, Some(4));
    assert!(high.source.unwrap().snapshot().frame.is_some());
    wait(
        &service,
        |event| matches!(event, Event::Player(audio) if audio.state == PlaybackState::Paused),
    );
    command(Command::ControlMv(MvAction::Playing(Some(false))));
    command(Command::ControlMv(MvAction::Quality(MvQuality::Standard)));
    let standard = completed();
    assert!(standard.error.is_none(), "{:?}", standard.error);
    assert_eq!(standard.quality, Some(MvQuality::Standard));
    command(Command::ControlMv(MvAction::Quality(MvQuality::Low)));
    let rollback = completed();
    assert_eq!(rollback.error, Some("该画质暂不可用"));
    assert_eq!(rollback.retry_quality, Some(MvQuality::Low));
    assert_eq!(rollback.quality, Some(MvQuality::Standard));
    let snapshot = rollback.source.unwrap().snapshot();
    assert!(snapshot.frame.is_some());
    assert_eq!(snapshot.state, PlaybackState::Paused);
    command(Command::CloseMv);
    wait(&service, |event| matches!(event, Event::Mv(None)));
    wait(
        &service,
        |event| matches!(event, Event::Player(audio) if audio.state == PlaybackState::Playing),
    );
    drop(service);
    running.store(false, Ordering::Release);
    mock.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
