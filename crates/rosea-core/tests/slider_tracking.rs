use rosea_core::{
    error::RoseaError,
    model::{ObjectId, Point, Stats, TimeMs},
    parse::osu,
    prepare::{SliderEventKind, prepare_map},
    replay::ButtonState,
    sim::{
        input::{InputAction, InputEvent},
        slider::{SliderState, tracking_at_sample},
    },
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

#[test]
fn head_timeout_is_once_at_fractional_deadline_and_body_can_be_captured() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000.25,2,0,L|100:0,1,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let mut stats = Stats {
        score: 800,
        current_combo: 3,
        max_combo: 5,
        ..Stats::default()
    };
    let original = stats;
    let windows = map.hit_windows();
    assert!(!state.timeout_head(TimeMs::new(1120.0).unwrap(), windows, &mut stats));
    assert_eq!(stats, original);
    assert!(state.timeout_head(TimeMs::new(1120.25).unwrap(), windows, &mut stats));
    assert_eq!(stats.score, 800);
    assert_eq!(stats.current_combo, 0);
    assert_eq!(stats.max_combo, 5);
    assert_eq!(stats.counts, original.counts);
    stats.current_combo = 2;
    assert!(!state.timeout_head(TimeMs::new(1121.0).unwrap(), windows, &mut stats));
    assert_eq!(stats.current_combo, 2);
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let cursor = object.position_at_time(1121.0).unwrap();
    assert!(
        state
            .sample(TimeMs::new(1121.0).unwrap(), cursor, buttons, 10.0)
            .unwrap()
    );
    assert!(
        !state
            .hit_head(1120, object.position(), buttons, windows, 10.0, &mut stats)
            .unwrap()
    );
    assert_eq!(stats.score, 800);
    assert_eq!(stats.current_combo, 2);
}

#[test]
fn hit_head_is_not_missed_at_deadline() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let mut state = SliderState::new(&map.objects()[0]).unwrap();
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let mut stats = Stats::default();
    assert!(
        state
            .hit_head(
                1000,
                Point { x: 0.0, y: 0.0 },
                buttons,
                map.hit_windows(),
                10.0,
                &mut stats
            )
            .unwrap()
    );
    assert!(!state.timeout_head(TimeMs::new(1120.0).unwrap(), map.hit_windows(), &mut stats));
    assert_eq!(stats.current_combo, 1);
    assert_eq!(stats.score, 30);
}

#[test]
fn ticks_follow_tracking_and_are_applied_once() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|300:0,1,300";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let mut stats = Stats::default();
    assert_eq!(state.total_parts(), 4);
    assert!(
        state
            .hit_head(
                1000,
                object.position(),
                buttons,
                map.hit_windows(),
                10.0,
                &mut stats
            )
            .unwrap()
    );
    let first = object.position_at_time(1500.0).unwrap();
    assert!(
        state
            .sample(TimeMs::new(1500.0).unwrap(), first, buttons, 10.0)
            .unwrap()
    );
    assert_eq!(state.tick(0, &mut stats), Ok(Some(true)));
    assert_eq!(state.successful_parts(), 2);
    assert_eq!(stats.score, 40);
    assert_eq!(stats.current_combo, 2);
    assert!(
        !state
            .sample(TimeMs::new(2000.0).unwrap(), first, buttons, 10.0)
            .unwrap()
    );
    assert_eq!(state.tick(1, &mut stats), Ok(Some(false)));
    assert_eq!(stats.score, 40);
    assert_eq!(stats.current_combo, 0);
    assert_eq!(stats.max_combo, 2);
    assert_eq!(stats.counts, Default::default());
    assert_eq!(state.successful_parts(), 2);
    assert_eq!(state.tick(0, &mut stats), Ok(None));
    assert_eq!(state.tick(2, &mut stats), Ok(None));
    assert_eq!(state.tick(100, &mut stats), Ok(None));
    assert_eq!(stats.score, 40);
}

#[test]
fn failed_tick_can_be_retried_without_partial_updates() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|200:0,1,200";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let buttons = ButtonState {
        left: false,
        right: true,
    };
    assert!(
        state
            .sample(
                TimeMs::new(1500.0).unwrap(),
                object.position_at_time(1500.0).unwrap(),
                buttons,
                10.0
            )
            .unwrap()
    );
    let mut stats = Stats {
        score: u64::MAX,
        ..Stats::default()
    };
    let original = stats;
    assert_eq!(state.tick(0, &mut stats), Err(RoseaError::ScoreOverflow));
    assert_eq!(stats, original);
    assert_eq!(state.successful_parts(), 0);
    stats.score = 0;
    assert_eq!(state.tick(0, &mut stats), Ok(Some(true)));
    assert_eq!(stats.score, 10);
    assert_eq!(state.successful_parts(), 1);
}

#[test]
fn repeat_uses_alternating_path_and_is_applied_once() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:0.01\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,3,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let mut stats = Stats::default();
    assert!(
        state
            .hit_head(
                1000,
                object.position(),
                buttons,
                map.hit_windows(),
                10.0,
                &mut stats
            )
            .unwrap()
    );

    let repeat_position = object.position_at_time(1500.0).unwrap();
    assert_eq!(repeat_position, Point { x: 100.0, y: 0.0 });
    assert!(
        state
            .sample(TimeMs::new(1500.0).unwrap(), repeat_position, buttons, 10.0)
            .unwrap()
    );
    assert_eq!(state.repeat(0, &mut stats), Ok(Some(true)));
    assert_eq!(state.repeat(0, &mut stats), Ok(None));
    let second_repeat_position = object.position_at_time(2000.0).unwrap();
    assert_eq!(second_repeat_position, Point { x: 0.0, y: 0.0 });
    assert!(
        state
            .sample(
                TimeMs::new(2000.0).unwrap(),
                second_repeat_position,
                buttons,
                10.0
            )
            .unwrap()
    );
    assert_eq!(state.repeat(1, &mut stats), Ok(Some(true)));
    assert_eq!(state.repeat(1, &mut stats), Ok(None));
    assert_eq!(state.successful_parts(), 3);
    assert_eq!(stats.score, 90);
    assert_eq!(stats.current_combo, 3);

    assert_eq!(state.repeat(2, &mut stats), Ok(None));
    let last_event = object.slider().unwrap().timing().events.len() as u32;
    assert_eq!(state.repeat(last_event, &mut stats), Ok(None));
    assert_eq!(stats.score, 90);
}

#[test]
fn missed_repeat_resets_combo_and_overflow_can_be_retried() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:0.01\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,2,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let mut stats = Stats {
        score: u64::MAX,
        current_combo: 4,
        max_combo: 4,
        ..Stats::default()
    };
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    assert!(
        state
            .sample(
                TimeMs::new(1500.0).unwrap(),
                object.position_at_time(1500.0).unwrap(),
                buttons,
                10.0
            )
            .unwrap()
    );
    assert_eq!(state.repeat(0, &mut stats), Err(RoseaError::ScoreOverflow));
    assert_eq!(state.successful_parts(), 0);
    stats.score = 0;
    assert_eq!(state.repeat(0, &mut stats), Ok(Some(true)));
    assert_eq!(stats.score, 30);

    let mut missed = SliderState::new(object).unwrap();
    let mut stats = Stats {
        current_combo: 3,
        max_combo: 3,
        ..Stats::default()
    };
    assert_eq!(missed.repeat(0, &mut stats), Ok(Some(false)));
    assert_eq!(missed.repeat(0, &mut stats), Ok(None));
    assert_eq!(stats.score, 0);
    assert_eq!(stats.current_combo, 0);
    assert_eq!(stats.max_combo, 3);
    assert_eq!(stats.counts.count_miss, 0);
    assert_eq!(missed.successful_parts(), 0);
}

#[test]
fn tail_check_uses_prepared_time_and_current_ball_once() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:0.01\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|300:0,3,300";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let slider = object.slider().unwrap();
    let tail_index = slider
        .timing()
        .events
        .iter()
        .position(|event| event.kind == SliderEventKind::TailCheck)
        .unwrap() as u32;
    let tail = &slider.timing().events[tail_index as usize];
    assert_eq!(tail.time, slider.timing().tail_check_time);
    assert!(tail.position.x < slider.timing().end_position.x - 1.0);

    let mut state = SliderState::new(object).unwrap();
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let time = TimeMs::new(tail.time).unwrap();
    assert!(state.sample(time, tail.position, buttons, 1.0).unwrap());
    let mut stats = Stats {
        current_combo: 1,
        max_combo: 1,
        ..Stats::default()
    };
    assert_eq!(state.tail_check(tail_index, &mut stats), Ok(Some(true)));
    assert_eq!(state.tail_check(tail_index, &mut stats), Ok(None));
    assert_eq!(state.successful_parts(), 1);
    assert_eq!(stats.score, 30);
    assert_eq!(stats.current_combo, 2);

    let other_part = slider
        .timing()
        .events
        .iter()
        .position(|event| event.kind != SliderEventKind::TailCheck)
        .unwrap() as u32;
    assert_eq!(state.tail_check(other_part, &mut stats), Ok(None));
    assert_eq!(stats.score, 30);
}

#[test]
fn missed_tail_keeps_combo_and_overflow_can_be_retried() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:0.01\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let tail_index = object
        .slider()
        .unwrap()
        .timing()
        .events
        .iter()
        .position(|event| event.kind == SliderEventKind::TailCheck)
        .unwrap() as u32;
    let mut stats = Stats {
        score: u64::MAX,
        current_combo: 3,
        max_combo: 3,
        ..Stats::default()
    };
    assert_eq!(state.tail_check(tail_index, &mut stats), Ok(Some(false)));
    assert_eq!(state.tail_check(tail_index, &mut stats), Ok(None));
    assert_eq!(stats.score, u64::MAX);
    assert_eq!(stats.current_combo, 3);
    assert_eq!(stats.counts.count_miss, 0);

    let mut successful = SliderState::new(object).unwrap();
    let tail = &object.slider().unwrap().timing().events[tail_index as usize];
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    assert!(
        successful
            .sample(TimeMs::new(tail.time).unwrap(), tail.position, buttons, 1.0)
            .unwrap()
    );
    let mut stats = Stats {
        score: u64::MAX,
        current_combo: 2,
        max_combo: 2,
        ..Stats::default()
    };
    assert_eq!(
        successful.tail_check(tail_index, &mut stats),
        Err(RoseaError::ScoreOverflow)
    );
    assert_eq!(successful.successful_parts(), 0);
    stats.score = 0;
    assert_eq!(
        successful.tail_check(tail_index, &mut stats),
        Ok(Some(true))
    );
    assert_eq!(stats.score, 30);
}

#[test]
fn record_end_events_refresh_tracking_after_each_record_and_eof_releases_it() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:0.01\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let mut stats = Stats {
        current_combo: 2,
        max_combo: 2,
        ..Stats::default()
    };
    assert!(state.timeout_head(TimeMs::new(1120.0).unwrap(), map.hit_windows(), &mut stats));
    assert!(!state.head_hit());

    let cursor = object.position_at_time(1121.0).unwrap();
    let held = ButtonState {
        left: true,
        right: false,
    };
    assert_eq!(
        state.observe_input_event(
            TimeMs::new(1121.0).unwrap(),
            InputEvent::Record {
                index: 0,
                cursor,
                buttons: held,
            },
            10.0,
        ),
        Ok(None)
    );
    assert_eq!(
        state.observe_input_event(
            TimeMs::new(1121.0).unwrap(),
            InputEvent::Press {
                index: 0,
                cursor,
                action: InputAction::Left,
            },
            10.0,
        ),
        Ok(None)
    );
    assert_eq!(
        state.observe_input_event(
            TimeMs::new(1121.0).unwrap(),
            InputEvent::RecordEnd {
                index: 0,
                cursor,
                buttons: held,
            },
            10.0,
        ),
        Ok(Some(true))
    );

    let held_cursor = Point {
        x: cursor.x + 20.0,
        y: cursor.y,
    };
    assert_eq!(
        state.observe_input_event(
            TimeMs::new(1122.0).unwrap(),
            InputEvent::RecordEnd {
                index: 1,
                cursor: held_cursor,
                buttons: held,
            },
            10.0,
        ),
        Ok(Some(true))
    );
    let outside = Point {
        x: cursor.x + 25.0,
        y: cursor.y,
    };
    assert_eq!(
        state.observe_input_event(
            TimeMs::new(1123.0).unwrap(),
            InputEvent::RecordEnd {
                index: 2,
                cursor: outside,
                buttons: held,
            },
            10.0,
        ),
        Ok(Some(false))
    );
    assert_eq!(
        state.observe_input_event(
            TimeMs::new(1124.0).unwrap(),
            InputEvent::RecordEnd {
                index: 3,
                cursor,
                buttons: held,
            },
            10.0,
        ),
        Ok(Some(true))
    );
    assert_eq!(
        state.observe_input_event(
            TimeMs::new(1124.0).unwrap(),
            InputEvent::End { cursor },
            10.0,
        ),
        Ok(Some(false))
    );
    assert!(!state.tracking());
}

#[test]
fn successful_head_immediately_uses_the_expanded_tracking_radius() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:0.01\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,100";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let object = &map.objects()[0];
    let mut state = SliderState::new(object).unwrap();
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let click = Point { x: 10.0, y: 0.0 };
    let ball = object.position_at_time(1110.0).unwrap();
    assert_eq!(ball, Point { x: 22.0, y: 0.0 });
    assert!(!tracking_at_sample(false, buttons, click, ball, 10.0));
    assert!(tracking_at_sample(true, buttons, click, ball, 10.0));

    let mut stats = Stats::default();
    assert!(
        state
            .hit_head(1110, click, buttons, map.hit_windows(), 10.0, &mut stats)
            .unwrap()
    );
    assert!(state.tracking());
}

#[test]
fn map_event_group_samples_only_its_slider_once_for_start_and_parts() {
    let bytes = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|0:0,2,0\n100,0,1000,2,0,L|100:0,2,0";
    let map = prepare_map(&osu(bytes).unwrap().value).unwrap();
    let time = TimeMs::new(1000.0).unwrap();
    let events: Vec<_> = map
        .events()
        .iter()
        .filter(|event| event.time == time)
        .copied()
        .collect();
    let target_events: Vec<_> = events
        .iter()
        .filter(|event| event.object_id == ObjectId(0))
        .copied()
        .collect();
    assert!(target_events.len() > 1);

    let mut state = SliderState::new(&map.objects()[0]).unwrap();
    let buttons = ButtonState {
        left: true,
        right: false,
    };
    let cursor = Point { x: 0.0, y: 0.0 };
    assert_eq!(
        state.sample_for_map_events(time, cursor, buttons, 10.0, &events),
        Ok(Some(true))
    );
    assert!(state.tracking());
    assert_eq!(
        state.sample_for_map_events(time, cursor, ButtonState::default(), 10.0, &events),
        Ok(Some(true))
    );

    let other_events: Vec<_> = events
        .iter()
        .filter(|event| event.object_id == ObjectId(1))
        .copied()
        .collect();
    let mut other_state = SliderState::new(&map.objects()[1]).unwrap();
    assert_eq!(
        other_state.sample_for_map_events(time, cursor, buttons, 10.0, &target_events),
        Ok(None)
    );
    assert!(!other_state.tracking());
    assert_eq!(
        other_state.sample_for_map_events(time, cursor, buttons, 10.0, &other_events),
        Ok(Some(false))
    );
    assert!(!other_state.tracking());

    let end_only: Vec<_> = events
        .iter()
        .filter(|event| {
            event.object_id == ObjectId(0)
                && event.kind == rosea_core::prepare::MapEventKind::SliderEnd
        })
        .copied()
        .collect();
    let mut untouched = SliderState::new(&map.objects()[0]).unwrap();
    assert_eq!(
        untouched.sample_for_map_events(time, cursor, buttons, 10.0, &end_only),
        Ok(None)
    );
    assert!(!untouched.tracking());
}
