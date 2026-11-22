use super::prefetch::{PREFETCH_CAPACITY, PrefetchEntry};
use super::*;
use bo_music_api::{playback::AudioSource, transport::TransportOptions};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct MockState {
    snapshot: PlaybackSnapshot,
    defer_load: bool,
    fail_load: bool,
    fail_next: usize,
    loads: Vec<(String, Option<f64>, Option<f64>)>,
    load_paused: Vec<bool>,
    pauses: Vec<bool>,
    refreshes: Vec<(u64, String, Option<f64>, Option<f64>)>,
    seeks: Vec<f64>,
    stops: usize,
}
#[derive(Clone, Default)]
struct MockPlayer(Rc<RefCell<MockState>>);
impl Player for MockPlayer {
    fn load(&self, source: LoadSource) -> Result<(), MpvError> {
        let mut state = self.0.borrow_mut();
        if state.fail_load || state.fail_next > 0 {
            state.fail_next = state.fail_next.saturating_sub(1);
            return Err(MpvError::Closed);
        }
        state.load_paused.push(source.initially_paused);
        state
            .loads
            .push((source.address, source.start_seconds, source.end_seconds));
        if !state.defer_load {
            state.snapshot.loaded_count = state.snapshot.loaded_count.wrapping_add(1);
            state.snapshot.state = PlaybackState::Playing;
        } else {
            state.snapshot.state = PlaybackState::Loading;
        }
        Ok(())
    }
    fn refresh_recovery(&self, id: u64, source: LoadSource) -> Result<(), MpvError> {
        let mut state = self.0.borrow_mut();
        if !state.snapshot.recovering || state.snapshot.recovery_id != id {
            return Err(MpvError::StaleRecovery);
        }
        state
            .refreshes
            .push((id, source.address, source.start_seconds, source.end_seconds));
        Ok(())
    }
    fn stop(&self) -> Result<(), MpvError> {
        self.0.borrow_mut().stops += 1;
        Ok(())
    }
    fn pause(&self, value: bool) -> Result<(), MpvError> {
        self.0.borrow_mut().pauses.push(value);
        Ok(())
    }
    fn seek(&self, seconds: f64) -> Result<(), MpvError> {
        self.0.borrow_mut().seeks.push(seconds);
        Ok(())
    }
    fn volume(&self, _: f64) -> Result<(), MpvError> {
        Ok(())
    }
    fn audio_filter(&self, _: Option<&str>) -> Result<(), MpvError> {
        Ok(())
    }
    fn audio_filter_weights(&self, _: &str, _: &str) -> Result<(), MpvError> {
        Ok(())
    }
    fn snapshot(&self) -> PlaybackSnapshot {
        self.0.borrow().snapshot
    }
}

fn track(id: i64) -> Track {
    Track {
        id,
        title: id.to_string(),
        artist_text: String::new(),
        artists: vec![],
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 30,
        available_qualities: vec![],
        audio_variants: vec![],
        requires_vip: false,
        requires_purchase: false,
        lyrics: None,
        has_mv: false,
        comment_count: None,
        favorite_count: None,
        share_count: None,
    }
}

fn controller() -> (PlaybackCoordinator<MockPlayer>, MockPlayer, Session) {
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let player = MockPlayer::default();
    (
        PlaybackCoordinator::new(transport, player.clone()),
        player,
        session,
    )
}

fn resolved(ticket: ResolutionTicket, audition: bool) -> ResolvedTicket {
    let source = AudioSource {
        url: format!("https://example.test/{}", ticket.track.id),
        requested_quality: None,
        requested_variant: None,
        format: "mp3".into(),
        bitrate_kbps: 128,
        size_bytes: 0,
        downgraded: false,
    };
    let result = if audition {
        PlaybackResolution::Audition {
            source,
            start_seconds: 5,
            end_seconds: 10,
        }
    } else {
        PlaybackResolution::Playable(source)
    };
    ResolvedTicket {
        generation: ticket.generation,
        revision: ticket.revision,
        result: Ok(result),
        from_prefetch: false,
        cache_entry: None,
    }
}

fn recovery_result(ticket: RecoveryTicket, audition: bool) -> RecoveryResult {
    let original = resolved(ticket.ticket, audition);
    RecoveryResult {
        generation: original.generation,
        revision: original.revision,
        recovery_id: ticket.recovery_id,
        failures: ticket.failures,
        result: Some(original.result),
    }
}

fn quality_result(ticket: QualityTicket) -> QualityResult {
    let quality = ticket.ticket.quality;
    let mut original = resolved(ticket.ticket, false);
    if let Ok(PlaybackResolution::Playable(source)) = &mut original.result {
        source.requested_quality = quality;
        source.url.push_str("/new-quality");
    }
    QualityResult {
        generation: original.generation,
        revision: original.revision,
        sequence: ticket.sequence,
        result: Some(original.result),
    }
}

fn multi_quality_track(id: i64) -> Track {
    let mut track = track(id);
    track.available_qualities = vec![
        AudioQuality::Standard,
        AudioQuality::High,
        AudioQuality::Lossless,
    ];
    track
}

#[path = "playback_tests_1.rs"]
mod cases_1;
#[path = "playback_tests_2.rs"]
mod cases_2;
