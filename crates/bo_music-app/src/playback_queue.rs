//! Queue changes, persistence and account isolation.
use super::*;

impl<P: Player> PlaybackCoordinator<P> {
    pub fn set_mode(&mut self, mode: PlayMode) {
        self.queue.set_mode(mode);
    }
    pub fn set_quality(&mut self, quality: Option<AudioQuality>) {
        self.quality = quality;
    }

    /// Appending to an empty queue starts the first item.
    pub fn append(&mut self, track: Track) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        let empty = self.queue.current().is_none();
        if empty {
            self.invalidate()?;
        }
        if self.queue.append(track) && empty {
            Ok(self.ticket())
        } else {
            Ok(None)
        }
    }

    /// Play one item without replacing the current queue, reusing an existing entry when present.
    pub fn enqueue_and_play(
        &mut self,
        track: Track,
    ) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.enqueue_and_play_with_origin(track, None)
    }

    pub fn enqueue_and_play_with_origin(
        &mut self,
        track: Track,
        origin: Option<PlaybackOrigin>,
    ) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        let index = if track.id > 0 {
            self.queue
                .items()
                .iter()
                .position(|item| item.id == track.id)
        } else {
            None
        };
        let index = index.unwrap_or_else(|| {
            let index = self.queue.current_index().map_or(0, |current| current + 1);
            self.queue.insert_next(track);
            index
        });
        let ticket = self.select(index)?;
        self.set_origin(origin);
        Ok(ticket)
    }

    pub fn insert_next(
        &mut self,
        track: Track,
    ) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        let empty = self.queue.current().is_none();
        if empty {
            self.invalidate()?;
        }
        self.queue.insert_next(track);
        Ok(if empty { self.ticket() } else { None })
    }

    /// Removing the current item stops it and starts the queue's new current item.
    pub fn remove(&mut self, index: usize) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        if index >= self.queue.items().len() {
            return Ok(None);
        }
        let is_current = self.queue.current_index() == Some(index);
        if is_current {
            self.invalidate()?;
        }
        self.queue.remove(index);
        Ok(if is_current { self.ticket() } else { None })
    }

    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        self.queue.reorder(from, to)
    }

    /// Replace the queue and start resolving its selected item.
    pub fn replace(
        &mut self,
        tracks: Vec<Track>,
        index: usize,
    ) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.replace_with_origin(tracks, index, None)
    }

    pub fn replace_with_origin(
        &mut self,
        tracks: Vec<Track>,
        index: usize,
        origin: Option<PlaybackOrigin>,
    ) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.invalidate()?;
        self.set_origin(origin);
        self.resume = None;
        self.queue.replace(tracks, index);
        Ok(self.ticket())
    }

    pub fn select(&mut self, index: usize) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        if index >= self.queue.items().len() {
            return Ok(None);
        }
        self.invalidate()?;
        self.queue.move_to(index);
        Ok(self.ticket())
    }

    pub fn next_track(&mut self) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        if !self.queue.has_next() {
            return Ok(None);
        }
        self.invalidate()?;
        self.queue.move_next();
        Ok(self.ticket())
    }

    pub fn previous(&mut self) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        if !self.queue.has_previous() {
            return Ok(None);
        }
        self.invalidate()?;
        self.queue.previous();
        Ok(self.ticket())
    }

    pub fn stop(&mut self) -> Result<(), CoordinatorError> {
        self.invalidate()
    }

    /// Restore a previously loaded account snapshot without starting playback.
    pub fn restore_queue(&mut self, snapshot: QueueSnapshot) -> Result<(), CoordinatorError> {
        self.invalidate()?;
        self.set_origin(None);
        self.queue
            .replace(snapshot.tracks, snapshot.current_index.unwrap_or(0));
        self.resume = self.queue.current().and_then(|track| {
            let position = snapshot.position_seconds;
            (position.is_finite()
                && position > 0.0
                && (track.duration_seconds <= 0
                    || f64::from(track.duration_seconds) - position > 10.0))
                .then_some((track.id, position))
        });
        Ok(())
    }

    /// Restore the current account's queue, without starting playback.
    /// Call this from a worker if disk access must not block the UI thread.
    pub fn restore_account_queue(
        &mut self,
        store: &QueueStore,
        enabled: bool,
    ) -> Result<(), QueuePersistenceError> {
        let identity = self.session.snapshot();
        let scope = if identity.is_authenticated() {
            identity.uid.as_str()
        } else {
            "anonymous"
        };
        let snapshot = if enabled {
            store.load(scope)?
        } else {
            QueueSnapshot::default()
        };
        if self.session.snapshot().revision != identity.revision {
            return Err(CoordinatorError::SessionChanged.into());
        }
        self.restore_queue(snapshot)?;
        if self.observed_revision != identity.revision
            || self.session.snapshot().revision != identity.revision
        {
            self.invalidate()?;
            self.queue.clear();
            self.resume = None;
            return Err(CoordinatorError::SessionChanged.into());
        }
        Ok(())
    }

    pub fn capture_queue(&self) -> Result<QueueCapture, CoordinatorError> {
        let identity = self.session.snapshot();
        if self.observed_revision != identity.revision {
            return Err(CoordinatorError::SessionChanged);
        }
        let snapshot = self
            .session
            .with_revision(identity.revision, || {
                let position = self
                    .resume
                    .filter(|(id, _)| self.queue.current().is_some_and(|track| track.id == *id))
                    .map(|(_, position)| position)
                    .unwrap_or_else(|| self.snapshot().position_seconds);
                QueueSnapshot::capture(&self.queue, position)
            })
            .ok_or(CoordinatorError::SessionChanged)?;
        let scope = if identity.is_authenticated() {
            identity.uid
        } else {
            "anonymous".into()
        };
        Ok(QueueCapture { scope, snapshot })
    }

    /// Save under the scope captured together with the queue, even if login
    /// changes while the disk write is in progress.
    pub fn save_account_queue(&self, store: &QueueStore) -> Result<(), QueuePersistenceError> {
        let capture = self.capture_queue()?;
        store.save(&capture.scope, &capture.snapshot)?;
        Ok(())
    }

    pub fn play_current(&mut self) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        self.ensure_account()?;
        self.invalidate()?;
        Ok(self.ticket())
    }

    pub(super) fn ensure_account(&mut self) -> Result<(), CoordinatorError> {
        if self.observed_revision != self.session.snapshot().revision {
            self.invalidate()?;
            self.set_origin(None);
            self.queue.clear();
            self.resume = None;
        }
        Ok(())
    }
}
