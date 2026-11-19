//! Playback queue: display order and shuffled playback order are separate.
use crate::track::Track;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlayMode {
    #[default]
    Sequential,
    ListLoop,
    SingleLoop,
    Shuffle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishAction {
    ReplayCurrent,
    PlayNext,
    Stop,
}

pub struct PlayQueue {
    items: Vec<Track>,
    order: Vec<usize>,
    cursor: Option<usize>,
    mode: PlayMode,
    random: u64,
    revision: u64,
}

impl Default for PlayQueue {
    fn default() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self::with_seed(time ^ SEQUENCE.fetch_add(1, Ordering::Relaxed))
    }
}

impl PlayQueue {
    /// A seed allows deterministic tests and restores; shuffling is not cryptographic.
    pub fn with_seed(seed: u64) -> Self {
        Self {
            items: Vec::new(),
            order: Vec::new(),
            cursor: None,
            mode: PlayMode::Sequential,
            random: if seed == 0 { 0x9e3779b97f4a7c15 } else { seed },
            revision: 0,
        }
    }
    pub fn items(&self) -> &[Track] {
        &self.items
    }
    pub fn mode(&self) -> PlayMode {
        self.mode
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn current_index(&self) -> Option<usize> {
        self.cursor.and_then(|i| self.order.get(i).copied())
    }
    pub fn current(&self) -> Option<&Track> {
        self.current_index().and_then(|i| self.items.get(i))
    }
    pub fn can_reorder(&self) -> bool {
        self.mode != PlayMode::Shuffle
    }
    pub fn has_next(&self) -> bool {
        self.cursor.is_some_and(|i| {
            !self.order.is_empty()
                && (matches!(self.mode, PlayMode::ListLoop | PlayMode::Shuffle)
                    || i + 1 < self.order.len())
        })
    }
    pub fn has_previous(&self) -> bool {
        self.cursor
            .is_some_and(|i| !self.order.is_empty() && (self.mode == PlayMode::ListLoop || i > 0))
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    fn random_below(&mut self, limit: usize) -> usize {
        // Xorshift is sufficient for playback ordering, never for secrets.
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        (self.random % limit as u64) as usize
    }
    fn identity_order(&mut self) {
        self.order = (0..self.items.len()).collect();
    }
    fn shuffle(&mut self, keep_first: Option<usize>, avoid_first: Option<usize>) {
        self.identity_order();
        for i in (1..self.order.len()).rev() {
            let j = self.random_below(i + 1);
            self.order.swap(i, j);
        }
        if let Some(first) = keep_first {
            if let Some(position) = self.order.iter().position(|&index| index == first) {
                self.order.swap(0, position);
            }
        } else if self.order.len() > 1 && Some(self.order[0]) == avoid_first {
            let position = self.random_below(self.order.len() - 1) + 1;
            self.order.swap(0, position);
        }
    }
    fn start_at(&mut self, index: usize) {
        if self.mode == PlayMode::Shuffle {
            self.shuffle(Some(index), None);
            self.cursor = Some(0);
        } else {
            self.identity_order();
            self.cursor = Some(index);
        }
    }
    pub fn set_mode(&mut self, mode: PlayMode) {
        if self.mode == mode {
            return;
        }
        let current = self.current_index();
        self.mode = mode;
        if let Some(current) = current {
            self.start_at(current);
        }
        self.changed();
    }
    pub fn replace(&mut self, items: Vec<Track>, start_index: usize) {
        self.items = items;
        if self.items.is_empty() {
            self.order.clear();
            self.cursor = None;
        } else {
            self.start_at(start_index.min(self.items.len() - 1));
        }
        self.changed();
    }
    pub fn append(&mut self, track: Track) -> bool {
        if track.id > 0 && self.items.iter().any(|item| item.id == track.id) {
            return false;
        }
        self.items.push(track);
        self.order.push(self.items.len() - 1);
        if self.cursor.is_none() {
            self.cursor = Some(0);
        }
        self.changed();
        true
    }
    pub fn append_range(&mut self, tracks: impl IntoIterator<Item = Track>) -> usize {
        let mut seen: HashSet<_> = self
            .items
            .iter()
            .filter_map(|track| (track.id > 0).then_some(track.id))
            .collect();
        let mut added = 0;
        for track in tracks {
            if track.id <= 0 || !seen.insert(track.id) {
                continue;
            }
            self.items.push(track);
            self.order.push(self.items.len() - 1);
            added += 1;
        }
        if added > 0 {
            if self.cursor.is_none() {
                self.cursor = Some(0);
            }
            self.changed();
        }
        added
    }
    pub fn insert_next(&mut self, track: Track) {
        let Some(cursor) = self.cursor else {
            self.append(track);
            return;
        };
        let index = self.current_index().expect("cursor is valid") + 1;
        self.items.insert(index, track);
        for item in &mut self.order {
            if *item >= index {
                *item += 1;
            }
        }
        self.order.insert(cursor + 1, index);
        self.changed();
    }
    pub fn move_to(&mut self, index: usize) -> bool {
        let Some(position) = self.order.iter().position(|&item| item == index) else {
            return false;
        };
        self.cursor = Some(position);
        self.changed();
        true
    }
    pub fn remove(&mut self, index: usize) -> bool {
        let Some(position) = self.order.iter().position(|&item| item == index) else {
            return false;
        };
        let cursor = self.cursor.expect("nonempty queue has a cursor");
        self.items.remove(index);
        self.order.remove(position);
        for item in &mut self.order {
            if *item > index {
                *item -= 1;
            }
        }
        self.cursor = if self.order.is_empty() {
            None
        } else if position < cursor {
            Some(cursor - 1)
        } else {
            Some(cursor.min(self.order.len() - 1))
        };
        self.changed();
        true
    }
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if !self.can_reorder() || from >= self.items.len() || to >= self.items.len() || from == to {
            return false;
        }
        let current = self.current_index().expect("nonempty queue has a cursor");
        let item = self.items.remove(from);
        self.items.insert(to, item);
        let current = if current == from {
            to
        } else if from < to && current > from && current <= to {
            current - 1
        } else if from > to && current >= to && current < from {
            current + 1
        } else {
            current
        };
        self.identity_order();
        self.cursor = Some(current);
        self.changed();
        true
    }
    /// Manual next always advances, including in single-track loop mode.
    pub fn move_next(&mut self) -> bool {
        if !self.has_next() {
            return false;
        }
        let cursor = self.cursor.expect("has next");
        self.cursor = Some(match self.mode {
            PlayMode::ListLoop => (cursor + 1) % self.order.len(),
            PlayMode::Shuffle if cursor + 1 == self.order.len() => {
                let previous = self.current_index();
                self.shuffle(None, previous);
                0
            }
            _ => cursor + 1,
        });
        self.changed();
        true
    }
    pub fn previous(&mut self) -> bool {
        if !self.has_previous() {
            return false;
        }
        let cursor = self.cursor.expect("has previous");
        self.cursor = Some(if self.mode == PlayMode::ListLoop {
            (cursor + self.order.len() - 1) % self.order.len()
        } else {
            cursor - 1
        });
        self.changed();
        true
    }
    pub fn peek_next(&self) -> Option<&Track> {
        let cursor = self.cursor?;
        let next = if self.mode == PlayMode::ListLoop {
            (cursor + 1) % self.order.len()
        } else {
            cursor + 1
        };
        self.order
            .get(next)
            .and_then(|&index| self.items.get(index))
    }
    pub fn on_finished(&mut self) -> FinishAction {
        if self.current().is_none() {
            FinishAction::Stop
        } else if self.mode == PlayMode::SingleLoop {
            FinishAction::ReplayCurrent
        } else if self.move_next() {
            FinishAction::PlayNext
        } else {
            FinishAction::Stop
        }
    }
    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.cursor = None;
        self.changed();
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
