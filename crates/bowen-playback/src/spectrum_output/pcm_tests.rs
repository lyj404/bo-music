use super::*;

#[test]
fn fragmented_pipe_reads_preserve_pcm_frame_alignment() {
    let mut bytes = Vec::new();
    for index in 0..3000 {
        let value = (std::f32::consts::TAU * index as f32 * 1_000.0 / RATE.get() as f32).sin();
        bytes.extend(value.to_le_bytes());
        bytes.extend(0.0f32.to_le_bytes());
    }
    let mut contiguous = PcmStream::new();
    contiguous.push(&bytes);
    let mut fragmented = PcmStream::new();
    fragmented.push(&bytes[..3]);
    assert!(fragmented.take_levels().is_none());
    for fragment in bytes[3..].chunks(83) {
        fragmented.push(fragment);
    }
    let levels = fragmented.take_levels().unwrap();
    assert_eq!(levels, contiguous.take_levels().unwrap());
    assert!(levels[25] > 0.85);
    assert!(fragmented.take_levels().is_none());
}
