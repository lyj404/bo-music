//! Cache and worker management for prefetched stream resolutions.
use super::*;

pub(super) const PREFETCH_TTL: Duration = Duration::from_secs(10 * 60);
pub(super) const PREFETCH_CAPACITY: usize = 8;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ResolutionKey {
    pub(super) revision: u64,
    pub(super) track: Track,
    pub(super) quality: Option<AudioQuality>,
}

pub(super) struct CachedEntry {
    pub(super) key: ResolutionKey,
    pub(super) started: Instant,
    // Failed prefetches are retained until selected to avoid repeated background retries.
    pub(super) result: Option<PlaybackResolution>,
}

pub(crate) struct PrefetchEntry {
    pub(super) key: ResolutionKey,
    pub(super) started: Instant,
    pub(super) task: tokio::task::JoinHandle<ResolvedTicket>,
}

impl Drop for PrefetchEntry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) enum PrefetchHit {
    Ready {
        result: PlaybackResolution,
        started: Instant,
    },
    Pending(PrefetchEntry),
}

#[derive(Default)]
pub(crate) struct Prefetcher {
    pub(super) entry: Option<PrefetchEntry>,
    pub(super) cache: Vec<CachedEntry>,
}

impl Prefetcher {
    pub(crate) fn cancel(&mut self) {
        self.entry = None;
        self.cache.clear();
    }
    pub(crate) fn retain_revision(&mut self, revision: u64) {
        self.cache.retain(|entry| entry.key.revision == revision);
        if self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.key.revision != revision)
        {
            self.entry = None;
        }
    }
    pub(super) fn prune(&mut self) {
        self.cache
            .retain(|entry| entry.started.elapsed() <= PREFETCH_TTL);
        if self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.started.elapsed() > PREFETCH_TTL)
        {
            self.entry = None;
        }
    }
    pub(super) fn insert(&mut self, item: CachedEntry) {
        self.cache.retain(|entry| entry.key != item.key);
        if self.cache.len() == PREFETCH_CAPACITY
            && let Some((oldest, _)) = self
                .cache
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.started)
        {
            self.cache.remove(oldest);
        }
        self.cache.push(item);
    }
    fn harvest(&mut self) {
        if !self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.task.is_finished())
        {
            return;
        }
        let mut entry = self.entry.take().expect("finished entry exists");
        let mut context = Context::from_waker(Waker::noop());
        let result = match Pin::new(&mut entry.task).poll(&mut context) {
            Poll::Ready(Ok(resolved)) => resolved.result.ok(),
            Poll::Ready(Err(_)) => None,
            Poll::Pending => {
                self.entry = Some(entry);
                return;
            }
        };
        if entry.started.elapsed() <= PREFETCH_TTL {
            self.insert(CachedEntry {
                key: entry.key.clone(),
                started: entry.started,
                result,
            });
        }
    }
    pub(crate) fn remember(&mut self, resolved: &mut ResolvedTicket, revision: u64) {
        if let Some(entry) = resolved.cache_entry.take()
            && entry.key.revision == revision
            && entry.started.elapsed() <= PREFETCH_TTL
        {
            self.insert(*entry);
        }
    }
    pub(crate) fn forget_for(&mut self, ticket: &ResolutionTicket) {
        self.harvest();
        let key = ticket.key();
        self.cache.retain(|entry| entry.key != key);
        if self.entry.as_ref().is_some_and(|entry| entry.key == key) {
            self.entry = None;
        }
    }
    pub(crate) fn reconcile(&mut self, candidate: Option<ResolutionTicket>) {
        self.harvest();
        self.prune();
        let Some(candidate) = candidate else {
            self.entry = None;
            return;
        };
        let key = candidate.key();
        if self.cache.iter().any(|entry| entry.key == key) {
            self.entry = None;
            return;
        }
        if self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.key == key && entry.started.elapsed() <= PREFETCH_TTL)
        {
            return;
        }
        self.entry = None;
        if self.cache.len() == PREFETCH_CAPACITY
            && let Some((oldest, _)) = self
                .cache
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.started)
        {
            self.cache.remove(oldest);
        }
        self.entry = Some(PrefetchEntry {
            key,
            started: Instant::now(),
            task: tokio::spawn(candidate.resolve()),
        });
    }
    pub(crate) fn take_for(&mut self, ticket: &ResolutionTicket) -> Option<PrefetchHit> {
        self.harvest();
        self.prune();
        let key = ticket.key();
        if let Some(index) = self.cache.iter().position(|entry| entry.key == key) {
            self.entry = None;
            if let Some(result) = &self.cache[index].result {
                return Some(PrefetchHit::Ready {
                    result: result.clone(),
                    started: self.cache[index].started,
                });
            }
            self.cache.remove(index);
            return None;
        }
        if self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.key == key && entry.started.elapsed() <= PREFETCH_TTL)
        {
            self.entry.take().map(PrefetchHit::Pending)
        } else {
            self.entry = None;
            None
        }
    }
}
