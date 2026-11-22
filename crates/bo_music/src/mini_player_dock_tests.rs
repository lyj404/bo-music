use super::*;

#[test]
fn dock_coordinates_respect_both_edges_of_a_negative_monitor() {
    let work = RECT {
        left: -1600,
        top: 0,
        right: -100,
        bottom: 900,
    };
    let left = RECT {
        left: -1595,
        right: -1120,
        ..work
    };
    let right = RECT {
        left: -580,
        right: -107,
        ..work
    };
    assert_eq!(Edge::near(left, work, 12), Some(Edge::Left));
    assert_eq!(Edge::near(right, work, 12), Some(Edge::Right));
    assert_eq!(Edge::near(right, work, 4), None);
    assert_eq!(Edge::Left.x(work, 32), -1600);
    assert_eq!(Edge::Right.x(work, 32), -132);
    assert_eq!(Edge::Right.x(work, 480), -580);
}
