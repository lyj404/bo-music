//! Collection status and mutations for the selected catalog item.

use super::*;

impl Browser {
    fn collection_target(&self) -> Option<CollectionTarget> {
        match self.selected.as_ref()? {
            Selection::Playlist(item) => Some(CollectionTarget::Playlist(
                item.id,
                if item.source_type > 0 {
                    item.source_type
                } else {
                    4
                },
            )),
            Selection::Album(item) => Some(CollectionTarget::Album(item.id)),
            Selection::Artist(item) => Some(CollectionTarget::Artist(item.id)),
            Selection::Chart(_) => None,
        }
    }

    pub fn load_collection_state(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        self.cancel_collection();
        let Some(target) = self.collection_target() else {
            return;
        };
        let identity = transport.session().snapshot();
        self.collection_state_revision = Some(identity.revision);
        if !identity.is_authenticated() {
            self.collection_state = CollectionState::LoginRequired;
            return;
        }
        if matches!(target, CollectionTarget::Playlist(_, 5)) {
            self.collection_state = CollectionState::Owned;
            return;
        }
        if self
            .collection_writes
            .contains(&(identity.revision, target))
        {
            self.collection_state = CollectionState::Writing;
            return;
        }
        self.collection_state = CollectionState::Loading;
        let generation = self.collection_generation;
        self.collection_task = Some(tokio::spawn(async move {
            let state = match target {
                CollectionTarget::Playlist(id, source) => {
                    match playlist::info(&transport, id, source).await {
                        Ok(Some(info))
                            if info.id == id
                                && info.creator_id > 0
                                && info.creator_id.to_string() == identity.uid =>
                        {
                            CollectionState::Owned
                        }
                        Ok(Some(info)) if info.id == id => {
                            CollectionState::Ready(!info.collect_time.is_empty())
                        }
                        _ => CollectionState::Unknown,
                    }
                }
                CollectionTarget::Album(id) => {
                    match collection::album_state(&transport, id).await {
                        Ok(Some(value)) => CollectionState::Ready(value),
                        _ => CollectionState::Unknown,
                    }
                }
                CollectionTarget::Artist(id) => {
                    match collection::followed_artists(&transport).await {
                        Ok(items) if items.iter().any(|item| item.id == id) => {
                            CollectionState::Ready(true)
                        }
                        Ok(items) if items.len() < 400 => CollectionState::Ready(false),
                        Ok(_) => CollectionState::Unknown,
                        Err(_) => CollectionState::Unknown,
                    }
                }
            };
            let _ = sender.send(Command::CollectionStateDone(
                generation,
                identity.revision,
                target,
                state,
            ));
        }));
    }

    pub fn finish_collection_state(
        &mut self,
        generation: u64,
        revision: u64,
        current: u64,
        target: CollectionTarget,
        state: CollectionState,
    ) -> bool {
        if generation != self.collection_generation
            || revision != current
            || self.collection_target() != Some(target)
        {
            return false;
        }
        self.collection_task = None;
        self.collection_state = state;
        true
    }

    pub fn write_collection(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        let Some(target) = self.collection_target() else {
            return;
        };
        let CollectionState::Ready(current) = self.collection_state else {
            return;
        };
        let revision = transport.session().snapshot().revision;
        if self.collection_state_revision != Some(revision) {
            self.collection_state = CollectionState::Unknown;
            return;
        }
        if !self.collection_writes.insert((revision, target)) {
            return;
        }
        self.collection_state = CollectionState::Writing;
        tokio::spawn(async move {
            let (id, source) = match target {
                CollectionTarget::Playlist(id, source) => (id, source),
                CollectionTarget::Album(id) => (id, 6),
                CollectionTarget::Artist(id) => (id, 7),
            };
            let desired = !current;
            let success = collection::set_at_revision(&transport, id, source, desired, revision)
                .await
                .is_ok();
            let _ = sender.send(Command::CollectionWriteDone(
                revision, target, desired, success,
            ));
        });
    }

    pub fn finish_collection_write(
        &mut self,
        revision: u64,
        current: u64,
        target: CollectionTarget,
        desired: bool,
        success: bool,
    ) -> bool {
        self.collection_writes.remove(&(revision, target));
        if revision != current {
            return false;
        }
        let mut list_changed = false;
        if success && !desired && self.collection {
            match target {
                CollectionTarget::Playlist(id, _) => {
                    let before = self.playlists.len();
                    self.playlists.retain(|item| item.id != id);
                    list_changed = before != self.playlists.len();
                }
                CollectionTarget::Album(id) => {
                    let before = self.albums.len();
                    self.albums.retain(|item| item.id != id);
                    list_changed = before != self.albums.len();
                }
                CollectionTarget::Artist(id) => {
                    let before = self.artists.len();
                    self.artists.retain(|item| item.id != id);
                    list_changed = before != self.artists.len();
                }
            }
        }
        if self.collection_target() != Some(target) {
            return list_changed;
        }
        self.collection_state = if success {
            CollectionState::Ready(desired)
        } else {
            CollectionState::Unknown
        };
        true
    }
}
