use rosea_core::{model::Point, replay::ButtonState, sim::slider::tracking_at_sample};

#[test]
fn capture_and_hold_use_different_radii() {
    let ball = Point { x: 0.0, y: 0.0 };
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let mut tracking = false;
    let mut states = Vec::new();
    for distance in [9.0, 20.0, 25.0, 20.0, 9.0] {
        tracking = tracking_at_sample(
            tracking,
            buttons,
            Point {
                x: distance,
                y: 0.0,
            },
            ball,
            10.0,
        );
        states.push(tracking);
    }
    assert_eq!(states, [true, true, false, false, true]);
}

#[test]
fn boundary_and_release_are_checked_at_each_sample() {
    let ball = Point { x: 0.0, y: 0.0 };
    let right = ButtonState {
        left: false,
        right: true,
    };
    let cursor = Point { x: 10.0, y: 0.0 };
    assert!(tracking_at_sample(false, right, cursor, ball, 10.0));
    assert!(tracking_at_sample(
        true,
        right,
        Point { x: 24.0, y: 0.0 },
        ball,
        10.0
    ));
    assert!(!tracking_at_sample(
        true,
        right,
        Point { x: 24.01, y: 0.0 },
        ball,
        10.0
    ));
    assert!(!tracking_at_sample(
        true,
        ButtonState::default(),
        ball,
        ball,
        10.0
    ));
    assert!(!tracking_at_sample(
        false,
        right,
        Point { x: 10.01, y: 0.0 },
        ball,
        10.0
    ));
}
