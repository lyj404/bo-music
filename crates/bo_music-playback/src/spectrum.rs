//! Logarithmic spectrum for interleaved floating-point PCM.
use std::f64::consts::TAU;
use std::num::{NonZeroU32, NonZeroUsize};

pub const SAMPLE_COUNT: usize = 2048;
pub const BAND_COUNT: usize = 48;
pub type SpectrumLevels = [f32; BAND_COUNT];

/// Keeps the latest PCM window. Capture and analysis belong on the same worker.
pub struct SpectrumAnalyzer {
    samples: [f32; SAMPLE_COUNT],
    window: [f64; SAMPLE_COUNT],
    bins: [(usize, usize); BAND_COUNT],
    fft: [(f64, f64); SAMPLE_COUNT],
    levels: SpectrumLevels,
    cursor: usize,
    dirty: bool,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: NonZeroU32) -> Self {
        let rate = f64::from(sample_rate.get());
        let upper_frequency = (rate / 2.0).min(16_000.0);
        let lower_frequency = upper_frequency.min(40.0);
        let bin = |band| {
            lower_frequency
                * (upper_frequency / lower_frequency).powf(band as f64 / BAND_COUNT as f64)
                * SAMPLE_COUNT as f64
                / rate
        };
        Self {
            samples: [0.0; SAMPLE_COUNT],
            window: std::array::from_fn(|index| {
                0.5 - 0.5 * (TAU * index as f64 / (SAMPLE_COUNT - 1) as f64).cos()
            }),
            bins: std::array::from_fn(|band| {
                let first = (bin(band).floor() as usize).clamp(1, SAMPLE_COUNT / 2);
                let last = (bin(band + 1).ceil() as usize).clamp(first, SAMPLE_COUNT / 2);
                (first, last)
            }),
            fft: [(0.0, 0.0); SAMPLE_COUNT],
            levels: [0.0; BAND_COUNT],
            cursor: 0,
            dirty: false,
        }
    }

    pub fn reset(&mut self) {
        self.samples.fill(0.0);
        self.levels.fill(0.0);
        self.cursor = 0;
        self.dirty = false;
    }

    pub fn append(&mut self, samples: &[f32], channels: NonZeroUsize) {
        let channels = channels.get();
        let frames = samples.len() / channels;
        let first = frames.saturating_sub(SAMPLE_COUNT) * channels;
        for frame in samples[first..].chunks_exact(channels) {
            let sample =
                frame.iter().map(|&sample| f64::from(sample)).sum::<f64>() / channels as f64;
            self.samples[self.cursor] = if sample.is_finite() {
                sample as f32
            } else {
                0.0
            };
            self.cursor = (self.cursor + 1) % SAMPLE_COUNT;
        }
        self.dirty |= frames > 0;
    }

    pub fn levels(&mut self) -> &SpectrumLevels {
        if self.dirty {
            self.analyze();
            self.dirty = false;
        }
        &self.levels
    }

    fn analyze(&mut self) {
        // Arrange windowed samples in bit-reversed order for the radix-2 FFT.
        for (index, &weight) in self.window.iter().enumerate() {
            let reversed = index.reverse_bits() >> (usize::BITS - SAMPLE_COUNT.ilog2());
            let sample = self.samples[(self.cursor + index) % SAMPLE_COUNT];
            self.fft[reversed] = (f64::from(sample) * weight, 0.0);
        }

        for stage in 1..=SAMPLE_COUNT.ilog2() {
            let length = 1 << stage;
            let (step_im, step_re) = (-TAU / length as f64).sin_cos();
            for block in self.fft.chunks_exact_mut(length) {
                let (left, right) = block.split_at_mut(length / 2);
                let (mut phase_re, mut phase_im) = (1.0, 0.0);
                for (left, right) in left.iter_mut().zip(right) {
                    let re = right.0 * phase_re - right.1 * phase_im;
                    let im = right.0 * phase_im + right.1 * phase_re;
                    *right = (left.0 - re, left.1 - im);
                    *left = (left.0 + re, left.1 + im);
                    (phase_re, phase_im) = (
                        phase_re * step_re - phase_im * step_im,
                        phase_re * step_im + phase_im * step_re,
                    );
                }
            }
        }

        for (level, &(first, last)) in self.levels.iter_mut().zip(&self.bins) {
            let power = self.fft[first..=last]
                .iter()
                .map(|&(re, im)| re * re + im * im)
                .fold(0.0, f64::max);
            let amplitude = power.sqrt() * 4.0 / SAMPLE_COUNT as f64;
            *level = ((20.0 * amplitude.max(0.0001).log10() + 80.0) / 80.0).clamp(0.0, 1.0) as f32;
        }
    }
}

#[cfg(test)]
#[path = "spectrum_tests.rs"]
mod tests;
