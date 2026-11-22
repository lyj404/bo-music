use super::*;

#[test]
fn interrupted_backdrop_fade_uses_the_currently_visible_palette() {
    let start = Instant::now();
    let from = [[20, 40, 60]; 3];
    let to = [[80, 60, 40]; 3];
    let fade = BackdropFade {
        from: Some(from),
        image: None,
        start,
    };
    let midpoint = start + BACKDROP_FADE / 2;
    let visible = fade.visible_palette(Some(to), midpoint);
    assert_eq!(visible, [[50, 50, 50]; 3]);

    let interrupted = BackdropFade {
        from: Some(visible),
        image: None,
        start: midpoint,
    };
    assert_eq!(interrupted.visible_palette(None, midpoint), visible);
    assert_eq!(
        interrupted.visible_palette(None, midpoint + BACKDROP_FADE),
        BASE_PALETTE
    );
}
