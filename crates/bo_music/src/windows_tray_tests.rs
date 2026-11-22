use super::*;

#[test]
fn embedded_icon_has_a_valid_small_resource() {
    let payload = icon_payload(ICON_BYTES, 16, 16).unwrap();
    assert!(payload.starts_with(b"\x89PNG\r\n\x1a\n") || payload.starts_with(&40u32.to_le_bytes()));
    assert!(icon_payload(&ICON_BYTES[..8], 16, 16).is_none());
}
