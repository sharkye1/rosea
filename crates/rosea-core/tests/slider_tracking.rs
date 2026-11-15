use rosea_core::{
    error::RoseaError,
    model::{ObjectId, Point, Stats, TimeMs},
    parse::osu,
    prepare::prepare_map,
    replay::ButtonState,
    sim::slider::{SliderState, tracking_at_sample},
};

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

#[test]
fn head_hit_uses_windows_prepared_path_and_is_counted_only_once() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,100\n300,0,2000,1,0";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    assert!(SliderState::new(&map.objects()[1]).is_none());
    assert_eq!(state.object_id(), ObjectId(0));
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let cursor = Point { x: 0.0, y: 0.0 };
    let mut stats = Stats::default();
    let windows = map.hit_windows();
    assert!(
        !state
            .hit_head(880, cursor, buttons, windows, 10.0, &mut stats)
            .unwrap()
    );
    assert!(
        !state
            .hit_head(
                1000,
                Point { x: 11.0, y: 0.0 },
                buttons,
                windows,
                10.0,
                &mut stats
            )
            .unwrap()
    );
    assert!(!state.head_hit());
    assert!(
        state
            .hit_head(1100, cursor, buttons, windows, 10.0, &mut stats)
            .unwrap()
    );
    assert!(state.head_hit());
    assert!(state.tracking());
    assert_eq!(stats.score, 30);
    assert_eq!(stats.current_combo, 1);
    assert_eq!(stats.counts, Default::default());
    assert!(
        !state
            .hit_head(1100, cursor, buttons, windows, 10.0, &mut stats)
            .unwrap()
    );
    assert_eq!(stats.score, 30);
    assert!(
        state
            .sample(
                TimeMs::new(1125.0).unwrap(),
                Point { x: 20.0, y: 0.0 },
                buttons,
                10.0
            )
            .unwrap()
    );
    assert!(
        !state
            .sample(TimeMs::new(1500.01).unwrap(), cursor, buttons, 10.0)
            .unwrap()
    );
}

#[test]
fn early_head_does_not_track_and_failed_score_does_not_change_state() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let mut state = SliderState::new(&map.objects()[0]).unwrap();
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let cursor = Point { x: 0.0, y: 0.0 };
    let mut stats = Stats {
        score: u64::MAX,
        ..Stats::default()
    };
    assert_eq!(
        state.hit_head(900, cursor, buttons, map.hit_windows(), 10.0, &mut stats),
        Err(RoseaError::ScoreOverflow)
    );
    assert!(!state.head_hit());
    assert!(!state.tracking());
    stats.score = 0;
    assert!(
        state
            .hit_head(900, cursor, buttons, map.hit_windows(), 10.0, &mut stats)
            .unwrap()
    );
    assert!(!state.tracking());
    assert!(
        !state
            .sample(TimeMs::new(999.0).unwrap(), cursor, buttons, 10.0)
            .unwrap()
    );
    assert!(
        state
            .sample(TimeMs::new(1000.0).unwrap(), cursor, buttons, 10.0)
            .unwrap()
    );
    assert!(
        !state
            .sample(
                TimeMs::new(1000.0).unwrap(),
                cursor,
                ButtonState::default(),
                10.0
            )
            .unwrap()
    );
}
