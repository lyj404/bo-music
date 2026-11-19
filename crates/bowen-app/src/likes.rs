//! Account-bound liked-song state for the current track.
use crate::desktop_service::Command;
use bowen_api::{catalog::service_cursor, playlist, transport::Transport};
use std::{collections::HashSet, sync::Arc};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LikeState {
    Empty,
    LoginRequired,
    Loading,
    NoPlaylist,
    Ready(bool),
    Writing,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LikeView {
    pub track_id: Option<i64>,
    pub state: LikeState,
}
impl Default for LikeView {
    fn default() -> Self {
        Self {
            track_id: None,
            state: LikeState::Empty,
        }
    }
}

pub struct LikedSnapshot {
    pub(crate) playlist: Option<i64>,
    pub(crate) ids: HashSet<i64>,
    pub(crate) complete: bool,
}
struct Cache {
    revision: u64,
    snapshot: LikedSnapshot,
    dirty: bool,
}

#[derive(Default)]
pub struct LikedSongs {
    key: Option<(u64, i64)>,
    view: LikeView,
    generation: u64,
    load: Option<JoinHandle<()>>,
    cache: Option<Cache>,
    writes: HashSet<(u64, i64)>,
}

async fn fetch(transport: &Transport, revision: u64) -> Result<LikedSnapshot, ()> {
    fetch_inner(transport, revision, None).await
}

pub(crate) async fn fetch_for_batch(
    transport: &Transport,
    revision: u64,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<LikedSnapshot, ()> {
    fetch_inner(transport, revision, Some(cancel)).await
}

async fn fetch_inner(
    transport: &Transport,
    revision: u64,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<LikedSnapshot, ()> {
    let canceled = || cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire));
    if canceled() {
        return Err(());
    }
    let playlist = playlist::liked_at_revision(transport, revision)
        .await
        .map_err(|_| ())?;
    let Some(playlist) = playlist else {
        return Ok(LikedSnapshot {
            playlist: None,
            ids: HashSet::new(),
            complete: true,
        });
    };
    let mut cursor = service_cursor(100);
    let mut ids = HashSet::new();
    // Bound requests even when the server repeats pages or omits totals.
    for _ in 0..100 {
        if canceled() {
            return Err(());
        }
        let page = playlist::tracks_at_revision(transport, playlist.id, 5, &mut cursor, revision)
            .await
            .map_err(|_| ())?;
        ids.extend(
            page.items
                .into_iter()
                .filter_map(|track| (track.id > 0).then_some(track.id)),
        );
        if cursor.exhausted() {
            return Ok(LikedSnapshot {
                playlist: Some(playlist.id),
                ids,
                complete: true,
            });
        }
    }
    Ok(LikedSnapshot {
        playlist: Some(playlist.id),
        ids,
        complete: false,
    })
}

impl LikedSongs {
    pub fn has_pending_writes(&self, revision: u64) -> bool {
        self.writes.iter().any(|(pending, _)| *pending == revision)
    }
    pub fn view(&self) -> LikeView {
        self.view
    }
    fn cancel_load(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(task) = self.load.take() {
            task.abort();
        }
    }
    pub fn reset(&mut self) {
        self.cancel_load();
        self.key = None;
        self.cache = None;
        self.view = LikeView::default();
    }
    pub fn select(
        &mut self,
        id: Option<i64>,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        let identity = transport.session().snapshot();
        let key = id.filter(|id| *id > 0).map(|id| (identity.revision, id));
        if self.key == key {
            return;
        }
        self.key = key;
        self.cancel_load();
        self.view = LikeView {
            track_id: key.map(|(_, id)| id),
            state: LikeState::Empty,
        };
        if key.is_none() {
            return;
        }
        if !identity.is_authenticated() {
            self.view.state = LikeState::LoginRequired;
            return;
        }
        self.refresh(transport, sender, false);
    }
    pub fn refresh(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        force: bool,
    ) {
        let Some((revision, id)) = self.key else {
            return;
        };
        self.cancel_load();
        let identity = transport.session().snapshot();
        if identity.revision != revision || !identity.is_authenticated() {
            self.view.state = LikeState::LoginRequired;
            return;
        }
        if self
            .writes
            .iter()
            .any(|(writing_revision, _)| *writing_revision == revision)
        {
            self.view.state = if self.writes.contains(&(revision, id)) {
                LikeState::Writing
            } else {
                LikeState::Loading
            };
            return;
        }
        if !force
            && let Some(cache) = &self.cache
            && cache.revision == revision
            && !cache.dirty
        {
            self.view.state = Self::state(&cache.snapshot, id);
            return;
        }
        self.view.state = LikeState::Loading;
        let generation = self.generation;
        self.load = Some(tokio::spawn(async move {
            let result = fetch(&transport, revision).await;
            let _ = sender.send(Command::LikedLoadDone(generation, revision, result));
        }));
    }
    fn state(snapshot: &LikedSnapshot, id: i64) -> LikeState {
        if snapshot.playlist.is_none() {
            LikeState::NoPlaylist
        } else if snapshot.ids.contains(&id) {
            LikeState::Ready(true)
        } else if snapshot.complete {
            LikeState::Ready(false)
        } else {
            LikeState::Unknown
        }
    }
    pub fn finish_load(
        &mut self,
        generation: u64,
        revision: u64,
        current: u64,
        result: Result<LikedSnapshot, ()>,
    ) -> bool {
        let Some((key_revision, id)) = self.key else {
            return false;
        };
        if generation != self.generation || revision != current || revision != key_revision {
            return false;
        }
        self.load = None;
        match result {
            Ok(snapshot) => {
                self.view.state = Self::state(&snapshot, id);
                self.cache = Some(Cache {
                    revision,
                    snapshot,
                    dirty: false,
                });
            }
            Err(()) => {
                self.cache = None;
                self.view.state = LikeState::Unknown;
            }
        }
        true
    }
    pub fn toggle(
        &mut self,
        id: i64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        let Some((revision, current_id)) = self.key else {
            return;
        };
        let LikeState::Ready(liked) = self.view.state else {
            return;
        };
        if current_id != id || transport.session().snapshot().revision != revision {
            return;
        }
        let Some(cache) = self
            .cache
            .as_mut()
            .filter(|cache| cache.revision == revision)
        else {
            return;
        };
        let Some(playlist_id) = cache.snapshot.playlist else {
            return;
        };
        if !self.writes.insert((revision, id)) {
            return;
        }
        cache.dirty = true;
        self.view.state = LikeState::Writing;
        tokio::spawn(async move {
            let desired = !liked;
            let success =
                playlist::set_tracks_at_revision(&transport, playlist_id, &[id], desired, revision)
                    .await
                    .is_ok();
            let _ = sender.send(Command::LikedWriteDone(revision, id, desired, success));
        });
    }
    pub fn finish_write(
        &mut self,
        revision: u64,
        current: u64,
        (id, desired, success): (i64, bool, bool),
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) -> bool {
        self.writes.remove(&(revision, id));
        if revision != current {
            return false;
        }
        let mut has_cache = false;
        if let Some(cache) = self
            .cache
            .as_mut()
            .filter(|cache| cache.revision == revision)
        {
            has_cache = true;
            if success {
                if desired {
                    cache.snapshot.ids.insert(id);
                } else {
                    cache.snapshot.ids.remove(&id);
                }
            }
            cache.dirty = true;
        }
        match self.key {
            Some((key_revision, key_id)) if key_revision == revision && key_id == id => {
                if success && !has_cache {
                    self.refresh(transport, sender, true);
                    return true;
                }
                self.view.state = if success {
                    LikeState::Ready(desired)
                } else {
                    LikeState::Unknown
                };
                true
            }
            Some((key_revision, _)) if key_revision == revision => {
                self.refresh(transport, sender, false);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "likes_tests.rs"]
mod tests;
