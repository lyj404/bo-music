use super::*;
#[test]
fn every_catalog_asset_is_embedded_with_expected_container() {
    for sound in bowen_domain::audio_effects::AMBIENT_SOUNDS {
        let (name, data) = asset(sound.id).unwrap();
        assert!(data.len() > 1024);
        assert!(if name.ends_with(".flac") {
            data.starts_with(b"fLaC")
        } else {
            data.starts_with(b"OggS")
        });
    }
    assert!(asset("texture.vinyl").unwrap().1.starts_with(b"OggS"));
    assert!(asset("../other").is_none());
}
