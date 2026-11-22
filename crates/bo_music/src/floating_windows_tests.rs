use super::*;

#[test]
fn placement_stays_within_a_smaller_monitor_with_negative_origin() {
    let screen = Bounds::new(point(px(-800.), px(-200.)), size(px(800.), px(600.)));
    let saved = Bounds::new(point(px(-120.), px(230.)), size(px(1100.), px(250.)));
    let result = constrain(saved, screen, size(px(400.), px(200.)));
    assert_eq!(
        result,
        Bounds::new(point(px(-800.), px(150.)), size(px(800.), px(250.)))
    );
}
