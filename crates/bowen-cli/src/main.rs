//! Opt-in CLI for API diagnostics and playback; no account credentials are stored.
use bowen_api::{
    catalog,
    search::{search_cursor, search_tracks},
    session::Session,
    transport::{Transport, TransportOptions},
};
use bowen_app::playback::{ApplyOutcome, PlaybackCoordinator, RecoveryOutcome, ResolutionTicket};
use bowen_playback::mpv::{LoadSource, MpvOptions, MpvPlayer, PlaybackState};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

fn usage() -> &'static str {
    "Usage:\n  bowen-cli search <32-digit-device-id> <keyword>\n  bowen-cli play <32-digit-device-id> <music-id> <libmpv-path>\n  bowen-cli play-local <libmpv-path> <audio-file> [--silent]\nUse the existing stable device ID; the CLI never generates or stores one."
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(usage)?;
    match command.as_str() {
        "search" => {
            let device = args.next().ok_or_else(usage)?;
            let keyword = args.next().ok_or_else(usage)?;
            if args.next().is_some() || !valid_device(&device) {
                return Err(usage().into());
            }
            let transport =
                Transport::new(TransportOptions::default(), Session::default(), device)?;
            let page = search_tracks(&transport, &keyword, &mut search_cursor(30)).await?;
            for track in page.items {
                println!("{}\t{}\t{}", track.id, track.title, track.artist_text);
            }
        }
        "play" => {
            let device = args.next().ok_or_else(usage)?;
            let track_id: i64 = args.next().ok_or_else(usage)?.parse()?;
            let library = args.next().ok_or_else(usage)?;
            if args.next().is_some() || !valid_device(&device) || track_id <= 0 {
                return Err(usage().into());
            }
            let transport = Arc::new(Transport::new(
                TransportOptions::default(),
                Session::default(),
                device,
            )?);
            let track = catalog::track(&transport, track_id)
                .await?
                .ok_or("track not found")?;
            println!("播放：{} - {}", track.title, track.artist_text);
            let player = MpvPlayer::open(&library, MpvOptions::default())?;
            let mut app = PlaybackCoordinator::new(transport, player);
            let ticket = app.replace(vec![track], 0)?.ok_or("empty queue")?;
            if begin_playback(&mut app, ticket).await? {
                println!("试听片段");
            }
            let started = Instant::now();
            loop {
                let state = app.snapshot();
                if state.state == PlaybackState::Failed {
                    return Err(format!("audio playback failed: {:?}", state.last_error).into());
                }
                if state.state == PlaybackState::Loading
                    && started.elapsed() > Duration::from_secs(30)
                {
                    return Err("audio loading timed out".into());
                }
                if let Some(ticket) = app.poll()? {
                    begin_playback(&mut app, ticket).await?;
                } else if state.state == PlaybackState::Stopped {
                    break;
                }
                if let Some(ticket) = app.recovery_ticket()
                    && let RecoveryOutcome::Denied(reason) =
                        app.apply_recovery(ticket.resolve().await)?
                {
                    return Err(format!("playback denied: {reason:?}").into());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        "play-local" => {
            let library = args.next().ok_or_else(usage)?;
            let path = args.next().ok_or_else(usage)?;
            let silent = match args.next().as_deref() {
                None => false,
                Some("--silent") => true,
                _ => return Err(usage().into()),
            };
            if args.next().is_some() || !Path::new(&path).is_file() {
                return Err(usage().into());
            }
            play(
                &library,
                LoadSource {
                    address: path,
                    start_seconds: None,
                    end_seconds: None,
                    initially_paused: false,
                },
                silent,
            )
            .await?;
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}

async fn begin_playback(
    app: &mut PlaybackCoordinator<MpvPlayer>,
    mut ticket: ResolutionTicket,
) -> Result<bool, Box<dyn std::error::Error>> {
    for _ in 0..2 {
        match app.apply(ticket.resolve().await)? {
            ApplyOutcome::Started => return Ok(false),
            ApplyOutcome::Audition => return Ok(true),
            ApplyOutcome::Denied(reason) => {
                return Err(format!("playback denied: {reason:?}").into());
            }
            ApplyOutcome::Stale => return Err("playback request was superseded".into()),
            ApplyOutcome::RetryFresh => ticket = app.play_current()?.ok_or("empty queue")?,
        }
    }
    Err("audio source could not be loaded".into())
}

fn valid_device(device: &str) -> bool {
    device.len() == 32
        && device
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

async fn play(
    library: &str,
    source: LoadSource,
    silent: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: silent.then(|| "null".into()),
        },
    )?;
    player.load(source)?;
    let started = Instant::now();
    loop {
        let state = player.snapshot();
        if state.completed_count > 0 {
            break;
        }
        if state.state == PlaybackState::Failed {
            return Err(format!("audio playback failed: {:?}", state.last_error).into());
        }
        if state.state == PlaybackState::Loading && started.elapsed() > Duration::from_secs(30) {
            return Err("audio loading timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}
