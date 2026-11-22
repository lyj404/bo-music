use crate::spectrum::{SpectrumAnalyzer, SpectrumLevels};
use std::num::{NonZeroU32, NonZeroUsize};

const CHANNELS: NonZeroUsize = NonZeroUsize::new(2).unwrap();
const RATE: NonZeroU32 = NonZeroU32::new(48_000).unwrap();
const FRAME_BYTES: usize = CHANNELS.get() * size_of::<f32>();

/// Decodes stereo f32 little-endian PCM without assuming pipe reads end at frame boundaries.
pub(super) struct PcmStream {
    analyzer: SpectrumAnalyzer,
    pending: Vec<u8>,
    samples: Vec<f32>,
    has_frames: bool,
}

impl PcmStream {
    pub fn new() -> Self {
        Self {
            analyzer: SpectrumAnalyzer::new(RATE),
            pending: Vec::new(),
            samples: Vec::new(),
            has_frames: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let complete = self.pending.len() / FRAME_BYTES * FRAME_BYTES;
        if complete == 0 {
            return;
        }
        self.samples.clear();
        self.samples.extend(
            self.pending[..complete]
                .chunks_exact(4)
                .map(|sample| f32::from_le_bytes(sample.try_into().unwrap())),
        );
        self.analyzer.append(&self.samples, CHANNELS);
        self.pending.copy_within(complete.., 0);
        self.pending.truncate(self.pending.len() - complete);
        self.has_frames = true;
    }

    pub fn take_levels(&mut self) -> Option<SpectrumLevels> {
        std::mem::take(&mut self.has_frames).then(|| *self.analyzer.levels())
    }
}

#[cfg(test)]
#[path = "pcm_tests.rs"]
mod tests;
