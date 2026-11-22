use super::*;

fn tone(rate: u32, frequency: u32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|index| {
            (TAU * f64::from(frequency) * index as f64 / f64::from(rate)).sin() as f32 * 0.5
        })
        .collect()
}

#[test]
fn tones_peak_in_the_expected_logarithmic_bands() {
    for (rate, frequency) in [(48_000, 1_000), (44_100, 4_000)] {
        let mut analyzer = SpectrumAnalyzer::new(NonZeroU32::new(rate).unwrap());
        analyzer.append(&tone(rate, frequency, SAMPLE_COUNT), NonZeroUsize::MIN);
        let levels = analyzer.levels();
        let (dominant, peak) = levels
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap();
        let expected =
            ((f64::from(frequency) / 40.0).ln() / 400.0_f64.ln() * BAND_COUNT as f64) as usize;
        assert!(dominant.abs_diff(expected) <= 1 && *peak > 0.85);
        assert!(levels[0] < 0.1 && levels[BAND_COUNT - 1] < 0.1);
    }
}

#[test]
fn chunked_stereo_and_large_mono_packets_use_the_same_latest_window() {
    let rate = NonZeroU32::new(48_000).unwrap();
    let mono = tone(rate.get(), 250, SAMPLE_COUNT + 317);
    let stereo = mono
        .iter()
        .flat_map(|&sample| [sample, sample])
        .collect::<Vec<_>>();
    let mut single = SpectrumAnalyzer::new(rate);
    let mut chunked = SpectrumAnalyzer::new(rate);
    single.append(&mono, NonZeroUsize::MIN);
    for packet in stereo.chunks(466) {
        chunked.append(packet, NonZeroUsize::new(2).unwrap());
    }
    assert_eq!(single.levels(), chunked.levels());
}

#[test]
fn silence_nonfinite_samples_and_reset_have_zero_levels() {
    let mut analyzer = SpectrumAnalyzer::new(NonZeroU32::new(48_000).unwrap());
    analyzer.append(&[f32::NAN; SAMPLE_COUNT], NonZeroUsize::MIN);
    assert_eq!(analyzer.levels(), &[0.0; BAND_COUNT]);
    analyzer.append(&tone(48_000, 1_000, SAMPLE_COUNT), NonZeroUsize::MIN);
    assert!(analyzer.levels().iter().any(|&level| level > 0.85));
    analyzer.reset();
    assert_eq!(analyzer.levels(), &[0.0; BAND_COUNT]);
}
