use rosea_core::{
    model::{HitCounts, Judgement, ObjectId, Point, Stats, TimeMs},
    parse::{osr, osu},
    prepare::{MapEventKind, prepare_map},
    replay::{ButtonState, normalize},
    sim::{
        EventKind, SimEvent, Simulator,
        input::{InputAction, InputEvent},
        timeline::{Timeline, TimelineEvent},
    },
};

fn replay_bytes() -> Vec<u8> {
    // LZMA-поток из пяти записей с двумя записями в одной миллисекунде.
    let compressed = [
        93, 0, 0, 4, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 31, 31, 4, 88, 49, 119, 133,
        199, 107, 63, 23, 15, 40, 77, 211, 55, 220, 66, 27, 182, 252, 131, 104, 180, 165, 99, 223,
        255, 147, 91, 32, 0,
    ];
    let mut bytes = vec![0];
    bytes.extend_from_slice(&20200427_i32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0]);
    for _ in 0..6 {
        bytes.extend_from_slice(&0_u16.to_le_bytes());
    }
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&(compressed.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&compressed);
    bytes
}

#[test]
fn merges_parsed_map_and_replay_without_repeating_events() {
    let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,-110,1,0\n100,0,-99.75,1,0\n200,0,-80,1,0\n300,0,-60,1,0";
    let map = prepare_map(&osu(map).unwrap().value).unwrap();
    let replay = normalize(osr(&replay_bytes()).unwrap().value.input)
        .unwrap()
        .value;
    let mut timeline = Timeline::new(&map, &replay);
    let mut observed = Vec::new();
    for time in [-1.0, 10.0, 10.0, 19.0, 20.25, 45.0, 50.0, 100.0] {
        timeline
            .advance_to(TimeMs::new(time).unwrap(), |time, event| {
                let item = match event {
                    TimelineEvent::Input(InputEvent::Record { index, .. }) => {
                        Some(("record", index))
                    }
                    TimelineEvent::Input(InputEvent::Press {
                        index,
                        action: InputAction::Left,
                        ..
                    }) => Some(("left", index)),
                    TimelineEvent::Input(InputEvent::Press {
                        index,
                        action: InputAction::Right,
                        ..
                    }) => Some(("right", index)),
                    TimelineEvent::Input(InputEvent::End { .. }) => Some(("eof", 0)),
                    TimelineEvent::Map(event) if event.kind == MapEventKind::CircleDeadline => {
                        Some(("deadline", event.object_id.0))
                    }
                    _ => None,
                };
                if let Some((kind, index)) = item {
                    observed.push((time.get(), kind, index));
                }
            })
            .unwrap();
    }
    assert_eq!(
        observed,
        [
            (0.0, "record", 0),
            (10.0, "record", 1),
            (10.0, "left", 1),
            (10.0, "record", 2),
            (10.0, "right", 2),
            (10.0, "deadline", 0),
            (20.25, "deadline", 1),
            (30.0, "record", 3),
            (40.0, "deadline", 2),
            (50.0, "record", 4),
            (50.0, "left", 4),
            (50.0, "eof", 0),
            (60.0, "deadline", 3),
        ]
    );
    assert_eq!(timeline.input().cursor(), Point { x: 40.0, y: 40.0 });
    assert_eq!(timeline.input().buttons(), ButtonState::default());
}

#[test]
fn parsed_inputs_can_finish_circle_simulation_after_replay_ends() {
    let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n10,10,10,1,0\n100,0,100,1,0";
    let map = prepare_map(&osu(map).unwrap().value).unwrap();
    let replay = normalize(osr(&replay_bytes()).unwrap().value.input)
        .unwrap()
        .value;
    let sim = Simulator::new(&map, &replay).unwrap();
    assert_eq!(sim.end_time().get(), 220.0);
    let mut events = Vec::new();
    let stats = sim.finish_with(&mut |event| events.push(*event)).unwrap();
    assert_eq!(
        stats,
        Stats {
            score: 300,
            current_combo: 0,
            max_combo: 1,
            counts: HitCounts {
                count_300: 1,
                count_100: 0,
                count_50: 0,
                count_miss: 1
            },
        }
    );
    assert_eq!(
        events,
        [
            SimEvent {
                time: TimeMs::new(10.0).unwrap(),
                object_id: ObjectId(0),
                kind: EventKind::Circle,
                part_index: None,
                part_hit: None,
                judgement: Some(Judgement::Great),
                score_delta: 300,
                combo_before: 0,
                combo_after: 1,
            },
            SimEvent {
                time: TimeMs::new(220.0).unwrap(),
                object_id: ObjectId(1),
                kind: EventKind::Circle,
                part_index: None,
                part_hit: None,
                judgement: Some(Judgement::Miss),
                score_delta: 0,
                combo_before: 1,
                combo_after: 0,
            },
        ]
    );
    assert_eq!(
        Simulator::new(&map, &replay).unwrap().finish().unwrap(),
        stats
    );
}

#[test]
fn simulator_finishes_short_spinner_and_circle() {
    let replay = normalize(osr(&replay_bytes()).unwrap().value.input)
        .unwrap()
        .value;
    let prefix = "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,0,1,0\n";
    let slider_map = format!("{prefix}100,0,100,2,0,L|200:0,1,100");
    let slider_map = prepare_map(&osu(slider_map.as_bytes()).unwrap().value).unwrap();
    assert!(Simulator::new(&slider_map, &replay).is_ok());

    let spinner_map = format!("{prefix}256,192,100,8,0,200");
    let spinner_map = prepare_map(&osu(spinner_map.as_bytes()).unwrap().value).unwrap();
    let stats = Simulator::new(&spinner_map, &replay)
        .unwrap()
        .finish()
        .unwrap();
    assert_eq!(
        stats.counts,
        HitCounts {
            count_300: 2,
            ..HitCounts::default()
        }
    );
}
