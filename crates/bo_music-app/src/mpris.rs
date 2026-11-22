//! Linux desktop media controls over the session bus.
use crate::{
    artwork::ArtworkFetcher,
    desktop_service::{Command, PlayerView},
};
use bo_music_domain::queue::PlayMode;
use bo_music_playback::mpv::PlaybackState;
use std::{collections::HashMap, io::Write, time::Instant};
use tokio::sync::{mpsc, watch};
use zbus::{
    connection, fdo,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Str, Value},
};

const BUS_NAME: &str = "org.mpris.MediaPlayer2.bo_music";
const OBJECT: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone, PartialEq, Eq)]
struct Track {
    account: u64,
    id: i64,
    title: String,
    artist: String,
    album: Option<String>,
    duration: i64,
    cover: Option<String>,
}

impl Track {
    fn path(&self) -> OwnedObjectPath {
        // Account revision keeps a request from the previous session stale after a switch.
        let path = format!(
            "{OBJECT}/track/a{}_t{}",
            self.account,
            self.id.unsigned_abs()
        );
        OwnedObjectPath::try_from(path).expect("numeric identifiers form a valid D-Bus path")
    }
}

#[derive(Clone)]
struct Snapshot {
    track: Option<Track>,
    state: PlaybackState,
    mode: PlayMode,
    volume: u8,
    next: bool,
    previous: bool,
    position: i64,
    observed_at: Instant,
    seek_serial: u64,
    art_url: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            track: None,
            state: PlaybackState::Idle,
            mode: PlayMode::Sequential,
            volume: 100,
            next: false,
            previous: false,
            position: 0,
            observed_at: Instant::now(),
            seek_serial: 0,
            art_url: None,
        }
    }
}

impl Snapshot {
    fn new(view: &PlayerView, seek_serial: u64) -> Self {
        Self {
            track: view.track_id.filter(|&id| id > 0).map(|id| Track {
                account: view.queue_account_revision,
                id,
                title: view.title.clone(),
                artist: view.artist.clone(),
                album: view.album.clone(),
                duration: micros(view.duration_ms, 1_000),
                cover: view
                    .cover_url
                    .as_ref()
                    .filter(|url| !url.is_empty() && url.len() <= 4096)
                    .cloned(),
            }),
            state: view.state,
            mode: view.mode,
            volume: view.volume,
            next: view.can_go_next,
            previous: view.can_go_previous,
            position: micros(view.position_ms, 1_000),
            observed_at: Instant::now(),
            seek_serial,
            art_url: None,
        }
    }

    fn status(&self) -> &'static str {
        match self.state {
            PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused",
            _ => "Stopped",
        }
    }

    fn loop_status(&self) -> &'static str {
        match self.mode {
            PlayMode::ListLoop => "Playlist",
            PlayMode::SingleLoop => "Track",
            _ => "None",
        }
    }

    fn shuffle(&self) -> bool {
        self.mode == PlayMode::Shuffle
    }
    fn volume(&self) -> f64 {
        f64::from(self.volume) / 100.
    }
    fn can_seek(&self) -> bool {
        self.track.as_ref().is_some_and(|track| track.duration > 0)
            && matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
    }
    fn can_pause(&self) -> bool {
        matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
    }

    fn position(&self) -> i64 {
        let Some(track) = &self.track else {
            return 0;
        };
        let position = if self.state == PlaybackState::Playing {
            self.position
                .saturating_add(self.observed_at.elapsed().as_micros().min(i64::MAX as u128) as i64)
        } else {
            self.position
        };
        if track.duration > 0 {
            position.min(track.duration)
        } else {
            position
        }
    }

    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let Some(track) = &self.track else {
            return HashMap::new();
        };
        let mut metadata = HashMap::from([
            (
                "mpris:trackid".into(),
                OwnedValue::from(ObjectPath::from(track.path())),
            ),
            ("xesam:title".into(), string_value(&track.title)),
        ]);
        if track.duration > 0 {
            metadata.insert("mpris:length".into(), track.duration.into());
        }
        if !track.artist.is_empty()
            && let Ok(artist) = OwnedValue::try_from(Value::from(vec![track.artist.clone()]))
        {
            metadata.insert("xesam:artist".into(), artist);
        }
        if let Some(album) = &track.album {
            metadata.insert("xesam:album".into(), string_value(album));
        }
        if let Some(url) = &self.art_url {
            metadata.insert("mpris:artUrl".into(), string_value(url));
        }
        metadata
    }

    fn relative_seek(&self, offset: i64) -> Option<Command> {
        let track = self.track.as_ref().filter(|_| self.can_seek())?;
        Some(Command::SeekFromMedia(track.account, track.id, offset))
    }

    fn absolute_seek(&self, path: &ObjectPath<'_>, position: i64) -> Option<Command> {
        let track = self.track.as_ref().filter(|_| self.can_seek())?;
        if track.path().as_str() != path.as_str() || !(0..=track.duration).contains(&position) {
            return None;
        }
        Some(Command::SeekTo(
            track.account,
            track.id,
            position as f64 / 1_000_000.,
        ))
    }
}

pub(super) struct Mpris {
    views: watch::Sender<Snapshot>,
    pending_seek: Option<(u64, i64)>,
}

impl Mpris {
    pub fn start(
        commands: mpsc::UnboundedSender<Command>,
        artwork: Option<ArtworkFetcher>,
    ) -> Self {
        let (views, updates) = watch::channel(Snapshot::default());
        tokio::spawn(async move {
            if let Err(error) = serve(commands, updates, artwork).await {
                eprintln!("MPRIS 系统媒体服务不可用：{error}");
            }
        });
        Self {
            views,
            pending_seek: None,
        }
    }

    pub fn note_seek(&mut self) {
        self.pending_seek = self
            .views
            .borrow()
            .track
            .as_ref()
            .map(|track| (track.account, track.id));
    }

    pub fn publish(&mut self, view: &PlayerView) {
        if self.views.is_closed() {
            return;
        }
        let seek = self.pending_seek.take().is_some_and(|(account, id)| {
            view.queue_account_revision == account && view.track_id == Some(id)
        });
        let serial = self
            .views
            .borrow()
            .seek_serial
            .wrapping_add(u64::from(seek));
        let _ = self.views.send(Snapshot::new(view, serial));
    }
}

struct Root(mpsc::UnboundedSender<Command>);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        let _ = self.0.send(Command::RaiseMainWindow);
    }
    fn quit(&self) {}
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "bo_music"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "bo_music"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<&str> {
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<&str> {
        Vec::new()
    }
}

struct Player {
    commands: mpsc::UnboundedSender<Command>,
    views: watch::Receiver<Snapshot>,
}

impl Player {
    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.send(Command::Next);
    }
    fn previous(&self) {
        self.send(Command::Previous);
    }
    fn pause(&self) {
        self.send(Command::SetPlaying(false));
    }
    fn play_pause(&self) {
        self.send(Command::Toggle);
    }
    fn stop(&self) {
        self.send(Command::Stop);
    }
    fn play(&self) {
        self.send(Command::SetPlaying(true));
    }
    fn seek(&self, offset: i64) {
        if let Some(command) = self.views.borrow().relative_seek(offset) {
            self.send(command);
        }
    }
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        if let Some(command) = self.views.borrow().absolute_seek(&track_id, position) {
            self.send(command);
        }
    }
    fn open_uri(&self, _uri: &str) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(
            "opening external URIs is unavailable".into(),
        ))
    }
    #[zbus(property)]
    fn playback_status(&self) -> &str {
        self.views.borrow().status()
    }
    #[zbus(property(emits_changed_signal = "invalidates"))]
    fn loop_status(&self) -> &str {
        self.views.borrow().loop_status()
    }
    #[zbus(property)]
    fn set_loop_status(&self, value: &str) -> zbus::Result<()> {
        let mode = match value {
            "None" if self.views.borrow().shuffle() => PlayMode::Shuffle,
            "None" => PlayMode::Sequential,
            "Playlist" => PlayMode::ListLoop,
            "Track" => PlayMode::SingleLoop,
            _ => return Err(fdo::Error::InvalidArgs("unrecognized loop status".into()).into()),
        };
        self.send(Command::SetPlayMode(mode));
        Ok(())
    }
    #[zbus(property(emits_changed_signal = "const"))]
    fn rate(&self) -> f64 {
        1.
    }
    #[zbus(property)]
    fn set_rate(&self, rate: f64) -> zbus::Result<()> {
        if rate == 1. {
            Ok(())
        } else {
            Err(fdo::Error::InvalidArgs("only normal playback rate is supported".into()).into())
        }
    }
    #[zbus(property(emits_changed_signal = "invalidates"))]
    fn shuffle(&self) -> bool {
        self.views.borrow().shuffle()
    }
    #[zbus(property)]
    fn set_shuffle(&self, enabled: bool) {
        if enabled {
            self.send(Command::SetPlayMode(PlayMode::Shuffle));
        } else if self.views.borrow().shuffle() {
            self.send(Command::SetPlayMode(PlayMode::Sequential));
        }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.views.borrow().metadata()
    }
    #[zbus(property(emits_changed_signal = "invalidates"))]
    fn volume(&self) -> f64 {
        self.views.borrow().volume()
    }
    #[zbus(property)]
    fn set_volume(&self, value: f64) -> zbus::Result<()> {
        if !value.is_finite() {
            return Err(fdo::Error::InvalidArgs("volume must be finite".into()).into());
        }
        self.send(Command::SetVolume(value.clamp(0., 1.) * 100.));
        Ok(())
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.views.borrow().position()
    }
    #[zbus(property(emits_changed_signal = "const"))]
    fn minimum_rate(&self) -> f64 {
        1.
    }
    #[zbus(property(emits_changed_signal = "const"))]
    fn maximum_rate(&self) -> f64 {
        1.
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.views.borrow().next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.views.borrow().previous
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.views.borrow().track.is_some()
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.views.borrow().can_pause()
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.views.borrow().can_seek()
    }
    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(signal)]
    async fn seeked(
        context: &zbus::object_server::SignalContext<'_>,
        position: i64,
    ) -> zbus::Result<()>;
}

async fn serve(
    commands: mpsc::UnboundedSender<Command>,
    mut updates: watch::Receiver<Snapshot>,
    artwork: Option<ArtworkFetcher>,
) -> zbus::Result<()> {
    let (published, views) = watch::channel(Snapshot::default());
    let connection = connection::Builder::session()?
        .serve_at(OBJECT, Root(commands.clone()))?
        .serve_at(OBJECT, Player { commands, views })?
        .name(BUS_NAME)?
        .build()
        .await?;
    let player = connection
        .object_server()
        .interface::<_, Player>(OBJECT)
        .await?;
    let mut previous = Snapshot::default();
    let mut cover = MediaArtwork::default();
    loop {
        let mut next = updates.borrow_and_update().clone();
        cover.select(next.track.as_ref(), artwork.as_ref());
        next.art_url = cover.ready.as_ref().map(|file| file.url.clone());
        let changes = changed_properties(&previous, &next);
        published.send_replace(next.clone());
        if !changes.is_empty() {
            connection
                .emit_signal(
                    None::<&str>,
                    OBJECT,
                    "org.freedesktop.DBus.Properties",
                    "PropertiesChanged",
                    &(PLAYER, changes, Vec::<&str>::new()),
                )
                .await?;
        }
        if previous.seek_serial != next.seek_serial
            && previous.track.as_ref().map(Track::path) == next.track.as_ref().map(Track::path)
        {
            Player::seeked(player.signal_context(), next.position).await?;
        }
        previous = next;
        tokio::select! {
            changed = updates.changed() => {
                if changed.is_err() { break; }
            }
            result = async { cover.job.as_mut().expect("a cover request is pending").await }, if cover.job.is_some() => {
                cover.job = None;
                cover.ready = result.ok().flatten();
            }
        }
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct CoverKey {
    account: u64,
    id: i64,
    url: String,
}

#[derive(Default)]
struct MediaArtwork {
    key: Option<CoverKey>,
    ready: Option<CoverFile>,
    job: Option<tokio::task::JoinHandle<Option<CoverFile>>>,
}

impl MediaArtwork {
    fn select(&mut self, track: Option<&Track>, fetcher: Option<&ArtworkFetcher>) {
        let key = track.and_then(|track| {
            Some(CoverKey {
                account: track.account,
                id: track.id,
                url: track.cover.clone()?,
            })
        });
        if self.key == key {
            return;
        }
        if let Some(job) = self.job.take() {
            job.abort();
        }
        self.ready = None;
        if let Some(fetcher) = fetcher.cloned()
            && let Some(key) = &key
        {
            let url = key.url.clone();
            self.job = Some(tokio::spawn(async move {
                let png = fetcher.fetch(&url).await.ok()?;
                tokio::task::spawn_blocking(move || CoverFile::write(&png))
                    .await
                    .ok()?
                    .ok()
            }));
        }
        self.key = key;
    }
}

impl Drop for MediaArtwork {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            job.abort();
        }
    }
}

struct CoverFile {
    _file: tempfile::NamedTempFile,
    url: String,
}

impl CoverFile {
    fn write(png: &[u8]) -> std::io::Result<Self> {
        let mut file = tempfile::Builder::new()
            .prefix("bo_music-mpris-")
            .suffix(".png")
            .tempfile()?;
        file.write_all(png)?;
        file.flush()?;
        let url = reqwest::Url::from_file_path(file.path())
            .map_err(|()| std::io::Error::other("invalid MPRIS cover path"))?
            .to_string();
        Ok(Self { _file: file, url })
    }
}

fn changed_properties(before: &Snapshot, after: &Snapshot) -> HashMap<&'static str, OwnedValue> {
    let mut changes = HashMap::new();
    if before.track != after.track || before.art_url != after.art_url {
        changes.insert("Metadata", after.metadata().into());
    }
    if before.status() != after.status() {
        changes.insert("PlaybackStatus", string_value(after.status()));
    }
    if before.loop_status() != after.loop_status() {
        changes.insert("LoopStatus", string_value(after.loop_status()));
    }
    if before.shuffle() != after.shuffle() {
        changes.insert("Shuffle", after.shuffle().into());
    }
    if before.volume != after.volume {
        changes.insert("Volume", after.volume().into());
    }
    for (name, old, new) in [
        ("CanGoNext", before.next, after.next),
        ("CanGoPrevious", before.previous, after.previous),
        ("CanPlay", before.track.is_some(), after.track.is_some()),
        ("CanPause", before.can_pause(), after.can_pause()),
        ("CanSeek", before.can_seek(), after.can_seek()),
    ] {
        if old != new {
            changes.insert(name, new.into());
        }
    }
    changes
}

fn micros(value: u64, factor: u64) -> i64 {
    value.saturating_mul(factor).min(i64::MAX as u64) as i64
}
fn string_value(value: &str) -> OwnedValue {
    OwnedValue::from(Str::from(value.to_owned()))
}

#[cfg(test)]
#[path = "mpris_tests.rs"]
mod tests;
