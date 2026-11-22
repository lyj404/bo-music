use super::*;

#[test]
fn wheel_preserves_rgb_and_rejects_corners() {
    for rgb in [0x000000, 0xffffff, 0x123456, 0xff0000, 0x00ff00, 0x0000ff] {
        let wheel = Wheel::from_rgb(rgb);
        assert_eq!(wheel.rgb(), rgb);
        assert!(wheel.point(0., 0.).is_none());
    }
    assert_eq!(
        Wheel::from_rgb(0xff0000).point(0.5, 0.5).unwrap().rgb(),
        0xffffff
    );
}
