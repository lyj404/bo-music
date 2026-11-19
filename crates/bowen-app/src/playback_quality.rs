//! Quality switching and network recovery for the active stream.
use super::*;

impl<P: Player> PlaybackCoordinator<P> {
    pub fn preferred_quality(&self) -> AudioQuality {
        self.quality.unwrap_or(AudioQuality::Lossless)
    }
    pub fn current_quality(&self) -> Option<CurrentQuality> {
        self.current_source.as_ref().map(|source| CurrentQuality {
            requested: source.requested_quality,
            format: source.format.clone(),
            bitrate_kbps: source.bitrate_kbps,
            downgraded: source.downgraded,
        })
    }
    pub fn available_qualities(&self) -> Vec<AudioQuality> {
        let Some(track) = self.queue.current() else {
            return Vec::new();
        };
        let mut result = track.available_qualities.clone();
        result.extend(track.audio_variants.iter().map(|variant| variant.quality));
        result.sort_unstable();
        result.dedup();
        result
    }
    pub fn changing_quality(&self) -> bool {
        self.quality_pending.is_some() || self.quality_load.is_some()
    }
    pub fn can_change_quality(&self) -> bool {
        let snapshot = self.player.snapshot();
        self.active
            && self.start_confirmed
            && self.audition.is_none()
            && !snapshot.recovering
            && self.quality_load.is_none()
            && matches!(
                snapshot.state,
                PlaybackState::Playing | PlaybackState::Paused
            )
    }
    pub(crate) fn quality_request_pending(&self) -> bool {
        self.quality_pending.is_some()
    }
    pub fn take_quality_outcome(&mut self) -> Option<QualityOutcome> {
        self.quality_outcome.take()
    }

    pub fn request_quality(
        &mut self,
        quality: AudioQuality,
    ) -> Result<Option<QualityTicket>, CoordinatorError> {
        self.ensure_account()?;
        let snapshot = self.player.snapshot();
        if self.quality_load.is_none()
            && self.current_source.as_ref().is_some_and(|source| {
                source.requested_quality == Some(quality) && !source.downgraded
            })
        {
            self.quality_sequence = self.quality_sequence.wrapping_add(1);
            self.quality_pending = None;
            return Ok(None);
        }
        if !self.active
            || !self.start_confirmed
            || self.audition.is_some()
            || self.quality_load.is_some()
            || snapshot.recovering
            || !matches!(
                snapshot.state,
                PlaybackState::Playing | PlaybackState::Paused
            )
            || !self.available_qualities().contains(&quality)
        {
            return Ok(None);
        }
        let Some(track) = self.queue.current().cloned() else {
            return Ok(None);
        };
        if self.current_source.is_none() {
            return Ok(None);
        }
        self.quality_sequence = self.quality_sequence.wrapping_add(1);
        self.quality_pending = Some(self.quality_sequence);
        Ok(Some(QualityTicket {
            ticket: ResolutionTicket {
                generation: self.generation,
                revision: self.observed_revision,
                track,
                quality: Some(quality),
                transport: Arc::clone(&self.transport),
            },
            sequence: self.quality_sequence,
        }))
    }

    pub fn cancel_quality_request(&mut self) {
        self.quality_pending = None;
    }

    pub fn apply_quality(
        &mut self,
        result: QualityResult,
    ) -> Result<QualityOutcome, CoordinatorError> {
        if self.quality_pending != Some(result.sequence)
            || self.generation != result.generation
            || self.session.snapshot().revision != result.revision
        {
            return Ok(QualityOutcome::Stale);
        }
        self.quality_pending = None;
        let snapshot = self.player.snapshot();
        if !self.active
            || self.audition.is_some()
            || snapshot.recovering
            || snapshot.completed_count != self.completed_count
            || !matches!(
                snapshot.state,
                PlaybackState::Playing | PlaybackState::Paused
            )
        {
            return Ok(QualityOutcome::Stale);
        }
        let Some(Ok(PlaybackResolution::Playable(source))) = result.result else {
            return Ok(QualityOutcome::Unavailable);
        };
        if !(source.url.starts_with("https://") || source.url.starts_with("http://"))
            || !matches!(
                source.format.to_ascii_lowercase().as_str(),
                "mp3" | "aac" | "ogg" | "flac"
            )
        {
            return Ok(QualityOutcome::Unavailable);
        }
        let Some(old_source) = self.current_source.clone() else {
            return Ok(QualityOutcome::Stale);
        };
        let load = LoadSource {
            address: source.url.clone(),
            start_seconds: Some(snapshot.position_seconds.max(0.0)),
            end_seconds: None,
            initially_paused: true,
        };
        let loaded = self
            .session
            .with_revision(result.revision, || self.player.load(load));
        let Some(loaded) = loaded else {
            return Ok(QualityOutcome::Stale);
        };
        let mut attempt = QualityLoad {
            old_source,
            phase: QualityPhase::Loading(source),
            loaded_count: snapshot.loaded_count,
            started: Instant::now(),
            position: snapshot.position_seconds.max(0.0),
            duration: snapshot.duration_seconds,
            paused: snapshot.state == PlaybackState::Paused,
            deferred_seek: None,
        };
        if loaded.is_err() && self.restore_quality_source(&mut attempt).is_err() {
            self.active = false;
            return Ok(QualityOutcome::Failed);
        }
        self.quality_load = Some(attempt);
        Ok(QualityOutcome::Loading)
    }

    pub(super) fn restore_quality_source(&self, attempt: &mut QualityLoad) -> Result<(), MpvError> {
        let state = self.player.snapshot();
        self.session
            .with_revision(self.observed_revision, || {
                self.player.load(LoadSource {
                    address: attempt.old_source.url.clone(),
                    start_seconds: Some(attempt.position),
                    end_seconds: None,
                    initially_paused: true,
                })
            })
            .ok_or(MpvError::StaleRecovery)??;
        attempt.loaded_count = state.loaded_count;
        attempt.started = Instant::now();
        attempt.phase = QualityPhase::Restoring;
        Ok(())
    }
    pub(crate) fn is_resolving(&self) -> bool {
        self.pending
    }

    /// Reauthorize after every two failed reloads, without consulting the prefetch cache.
    pub fn recovery_ticket(&mut self) -> Option<RecoveryTicket> {
        let snapshot = self.player.snapshot();
        if !self.active
            || !self.start_confirmed
            || !snapshot.recovering
            || self.observed_revision != self.session.snapshot().revision
        {
            self.recovery_pending = None;
            return None;
        }
        if self
            .recovery_pending
            .is_some_and(|(id, _)| id != snapshot.recovery_id)
        {
            self.recovery_pending = None;
        }
        if self.recovery_pending.is_some() || snapshot.recovery_failures < 2 {
            return None;
        }
        if self.recovery_last.is_some_and(|(id, failures)| {
            id == snapshot.recovery_id && snapshot.recovery_failures < failures.saturating_add(2)
        }) {
            return None;
        }
        let track = self.queue.current()?.clone();
        if track.id <= 0 {
            return None;
        }
        let marker = (snapshot.recovery_id, snapshot.recovery_failures);
        self.recovery_pending = Some(marker);
        self.recovery_last = Some(marker);
        Some(RecoveryTicket {
            ticket: ResolutionTicket {
                generation: self.generation,
                revision: self.observed_revision,
                track,
                quality: self
                    .current_source
                    .as_ref()
                    .and_then(|source| source.requested_quality)
                    .or(self.quality),
                transport: Arc::clone(&self.transport),
            },
            recovery_id: marker.0,
            failures: marker.1,
        })
    }

    pub fn cancel_recovery_refresh(&mut self) {
        self.recovery_pending = None;
    }

    pub fn apply_recovery(
        &mut self,
        resolved: RecoveryResult,
    ) -> Result<RecoveryOutcome, CoordinatorError> {
        if self.recovery_pending != Some((resolved.recovery_id, resolved.failures))
            || resolved.generation != self.generation
            || resolved.revision != self.session.snapshot().revision
        {
            return Ok(RecoveryOutcome::Stale);
        }
        self.recovery_pending = None;
        let snapshot = self.player.snapshot();
        if !self.active || !snapshot.recovering || snapshot.recovery_id != resolved.recovery_id {
            return Ok(RecoveryOutcome::Stale);
        }
        let Some(Ok(result)) = resolved.result else {
            return Ok(RecoveryOutcome::Unavailable);
        };
        let (source, audition) = match result {
            PlaybackResolution::Denied(reason) => {
                self.invalidate()?;
                return Ok(RecoveryOutcome::Denied(reason));
            }
            PlaybackResolution::Playable(source) => (source, None),
            PlaybackResolution::Audition {
                source,
                start_seconds,
                end_seconds,
            } if start_seconds >= 0 && end_seconds > start_seconds => (
                source,
                Some((f64::from(start_seconds), f64::from(end_seconds))),
            ),
            PlaybackResolution::Audition { .. } => return Err(CoordinatorError::InvalidAudition),
        };
        let load = LoadSource {
            address: source.url.clone(),
            start_seconds: audition.map(|range| range.0),
            end_seconds: audition.map(|range| range.1),
            initially_paused: snapshot.state == PlaybackState::Paused,
        };
        match self.session.with_revision(resolved.revision, || {
            self.player.refresh_recovery(resolved.recovery_id, load)
        }) {
            None | Some(Err(MpvError::StaleRecovery)) => Ok(RecoveryOutcome::Stale),
            Some(Err(error)) => Err(error.into()),
            Some(Ok(())) => {
                self.audition = audition;
                self.current_source = Some(source);
                Ok(RecoveryOutcome::Refreshed)
            }
        }
    }
    pub(crate) fn prefetch_candidate(&self) -> Option<ResolutionTicket> {
        if !self.active
            || !self.start_confirmed
            || self.observed_revision != self.session.snapshot().revision
        {
            return None;
        }
        let track = self.queue.peek_next()?.clone();
        if track.id <= 0 {
            return None;
        }
        Some(ResolutionTicket {
            generation: self.generation,
            revision: self.observed_revision,
            track,
            quality: self.quality,
            transport: Arc::clone(&self.transport),
        })
    }
}
