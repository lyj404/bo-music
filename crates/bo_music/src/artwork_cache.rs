//! Small UI-side cache for background artwork thumbnails.
use bo_music_app::artwork::{AmbientPalette, ArtworkThumbnail};
use gpui::{App, Image, ImageFormat};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 64;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING: usize = 32;
const RETRY_DELAY: Duration = Duration::from_secs(60);

enum State {
    Pending,
    Ready {
        image: Arc<Image>,
        bytes: usize,
        ambient: Option<AmbientPalette>,
    },
    Failed(Instant),
}
struct Entry {
    state: State,
    used: u64,
}

#[derive(Default)]
pub struct ArtworkCache {
    entries: HashMap<String, Entry>,
    sequence: u64,
    pending: usize,
    bytes: usize,
}
impl ArtworkCache {
    pub fn cached(&self, address: &str) -> Option<Arc<Image>> {
        match &self.entries.get(address)?.state {
            State::Ready { image, .. } => Some(Arc::clone(image)),
            State::Pending | State::Failed(_) => None,
        }
    }

    pub fn ambient(&self, address: &str) -> Option<AmbientPalette> {
        match &self.entries.get(address)?.state {
            State::Ready { ambient, .. } => *ambient,
            State::Pending | State::Failed(_) => None,
        }
    }

    pub fn get(
        &mut self,
        address: &str,
        cx: &mut App,
        queue: impl FnOnce(String) -> bool,
    ) -> Option<Arc<Image>> {
        self.sequence = self.sequence.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(address) {
            entry.used = self.sequence;
            match &entry.state {
                State::Ready { image, .. } => return Some(Arc::clone(image)),
                State::Pending => return None,
                State::Failed(at) if at.elapsed() < RETRY_DELAY => return None,
                State::Failed(_) => {}
            }
            self.entries.remove(address);
        }
        if self.pending >= MAX_PENDING {
            return None;
        }
        let state = if queue(address.to_owned()) {
            self.pending += 1;
            State::Pending
        } else {
            State::Failed(Instant::now())
        };
        self.entries.insert(
            address.to_owned(),
            Entry {
                state,
                used: self.sequence,
            },
        );
        self.prune(cx);
        None
    }

    pub fn finish(&mut self, address: &str, result: Result<ArtworkThumbnail, ()>, cx: &mut App) {
        let Some(entry) = self.entries.get_mut(address) else {
            return;
        };
        if !matches!(entry.state, State::Pending) {
            return;
        }
        self.pending = self.pending.saturating_sub(1);
        entry.state = match result {
            Ok(ArtworkThumbnail { png, ambient }) if !png.is_empty() && png.len() <= MAX_BYTES => {
                let bytes = png.len();
                self.bytes += bytes;
                State::Ready {
                    image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
                    bytes,
                    ambient,
                }
            }
            _ => State::Failed(Instant::now()),
        };
        self.prune(cx);
    }

    fn prune(&mut self, cx: &mut App) {
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
            let Some(key) = self
                .entries
                .iter()
                .filter(|(_, entry)| !matches!(entry.state, State::Pending))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(Entry {
                state: State::Ready { image, bytes, .. },
                ..
            }) = self.entries.remove(&key)
            {
                self.bytes = self.bytes.saturating_sub(bytes);
                image.remove_asset(cx);
            }
        }
    }
}
