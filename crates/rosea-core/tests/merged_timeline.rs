use rosea_core::{
    model::{Beatmap, HitCounts, Judgement, ObjectId, Point, Stats, TimeMs},
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
    replay_with_payload(&compressed)
}

fn replay_with_payload(compressed: &[u8]) -> Vec<u8> {
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
    bytes.extend_from_slice(compressed);
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

fn mixed_beatmap() -> Beatmap {
    let map = b"osu file format v14\n\
        [Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n\
        [TimingPoints]\n0,500,4,0,0,100,1,0\n\
        [HitObjects]\n300,0,1500.25,2,0,B|400:100|500:0,1,200\n\
        50,200,900.25,1,0\n\
        256,192,1250.25,8,0,2000.25\n\
        0,0,1000.25,2,0,L|200:0,1,200\n\
        0,300,1800.25,2,0,P|100:400|200:300,1,200";
    osu(map).unwrap().value
}

fn mixed_replay_bytes() -> Vec<u8> {
    // После круга и головы L курсор делает пять полуоборотов, затем
    // возвращается к тику L. Последняя запись нажимает голову B.
    let mut records = &b"900|50|200|1,10|50|200|0,90|0|0|1,250|356|192|1,1|356|192|1,\
        9|156|192|1,20|356|192|1,20|156|192|1,20|356|192|1,20|156|192|1,\
        159|100|0|1,2|100|0|1,9|300|0|3,"[..];
    let mut compressed = Vec::new();
    lzma_rs::lzma_compress(&mut records, &mut compressed).unwrap();
    replay_with_payload(&compressed)
}

#[test]
fn mixed_map_finishes_each_object_once_after_early_eof() {
    let parsed = mixed_beatmap();
    let map = prepare_map(&parsed).unwrap();
    let replay = osr(&mixed_replay_bytes()).unwrap().value;
    let input = normalize(replay.input).unwrap().value;
    let mut sim = Simulator::new(&map, &input).unwrap();
    let eof = input.records().last().unwrap().time_ms;
    assert_eq!(eof, 1510);
    assert_eq!(sim.end_time().get(), 2800.25);

    let mut journal = Vec::new();
    sim.advance_with(TimeMs::new(eof as f64).unwrap(), &mut |event| {
        journal.push(*event)
    })
    .unwrap();
    assert_eq!(
        sim.stats().counts,
        HitCounts {
            count_300: 1,
            ..HitCounts::default()
        }
    );
    let stats = sim.finish_with(&mut |event| journal.push(*event)).unwrap();

    use EventKind::{Circle, SliderResult, SliderTick, SpinnerResult};
    use Judgement::{Great, Meh, Miss, Ok};
    let simultaneous: Vec<_> = journal
        .iter()
        .filter(|event| event.time.get() == 2000.25)
        .map(|event| (event.object_id, event.kind, event.part_hit))
        .collect();
    assert_eq!(
        simultaneous,
        [
            (ObjectId(0), SliderTick, Some(false)),
            (ObjectId(2), SpinnerResult, None),
            (ObjectId(3), SliderResult, None),
        ]
    );
    let finals: Vec<_> = journal
        .iter()
        .filter_map(|event| {
            event
                .judgement
                .map(|grade| (event.time.get(), event.object_id, event.kind, grade))
        })
        .collect();
    assert_eq!(
        finals,
        [
            (900.0, ObjectId(1), Circle, Great),
            (2000.25, ObjectId(2), SpinnerResult, Great),
            (2000.25, ObjectId(3), SliderResult, Ok),
            (2500.25, ObjectId(0), SliderResult, Meh),
            (2800.25, ObjectId(4), SliderResult, Miss),
        ]
    );
    let counts = stats.counts;
    assert_eq!(
        counts,
        HitCounts {
            count_300: 2,
            count_100: 1,
            count_50: 1,
            count_miss: 1
        }
    );
    assert_eq!(
        counts.count_300 + counts.count_100 + counts.count_50 + counts.count_miss,
        parsed.object_count()
    );
}

#[test]
fn mixed_map_journal_keeps_order_without_changing_stats() {
    let map = prepare_map(&mixed_beatmap()).unwrap();
    let input = normalize(osr(&mixed_replay_bytes()).unwrap().value.input)
        .unwrap()
        .value;
    let mut journal = Vec::new();
    let with_journal = Simulator::new(&map, &input)
        .unwrap()
        .finish_with(&mut |event| journal.push(*event))
        .unwrap();
    let without_journal = Simulator::new(&map, &input).unwrap().finish().unwrap();
    assert_eq!(with_journal, without_journal);

    use EventKind::{
        Circle, SliderHead, SliderResult, SliderTail, SliderTick, SpinnerPoints, SpinnerResult,
    };
    let observed: Vec<_> = journal
        .iter()
        .map(|event| (event.time.get(), event.object_id, event.kind))
        .collect();
    assert_eq!(
        observed,
        [
            (900.0, ObjectId(1), Circle),
            (1000.0, ObjectId(3), SliderHead),
            (1280.0, ObjectId(2), SpinnerPoints),
            (1320.0, ObjectId(2), SpinnerPoints),
            (1500.25, ObjectId(3), SliderTick),
            (1510.0, ObjectId(0), SliderHead),
            (1920.25, ObjectId(4), SliderHead),
            (1964.25, ObjectId(3), SliderTail),
            (2000.25, ObjectId(0), SliderTick),
            (2000.25, ObjectId(2), SpinnerResult),
            (2000.25, ObjectId(3), SliderResult),
            (2300.25, ObjectId(4), SliderTick),
            (2464.25, ObjectId(0), SliderTail),
            (2500.25, ObjectId(0), SliderResult),
            (2764.25, ObjectId(4), SliderTail),
            (2800.25, ObjectId(4), SliderResult),
        ]
    );
    assert_eq!(
        journal.iter().map(|event| event.score_delta).sum::<u64>(),
        with_journal.score
    );
}

#[test]
fn mixed_map_stats_and_journal_are_independent_of_advance_frequency() {
    let map = prepare_map(&mixed_beatmap()).unwrap();
    let input = normalize(osr(&mixed_replay_bytes()).unwrap().value.input)
        .unwrap()
        .value;
    let baseline = Simulator::new(&map, &input).unwrap();
    let end = baseline.end_time().get();
    let mut expected_journal = Vec::new();
    let expected_stats = baseline
        .finish_with(&mut |event| expected_journal.push(*event))
        .unwrap();

    let check = |times: &[f64], schedule: &str| {
        let mut sim = Simulator::new(&map, &input).unwrap();
        let mut journal = Vec::new();
        let mut emit = |event: &SimEvent| journal.push(*event);
        for &time in times {
            sim.advance_with(TimeMs::new(time).unwrap(), &mut emit)
                .unwrap();
        }
        let stats = sim.finish_with(&mut emit).unwrap();
        assert_eq!(stats, expected_stats, "{schedule}");
        assert_eq!(journal, expected_journal, "{schedule}");
    };
    for fps in [30, 60, 144] {
        let times: Vec<_> = (0..)
            .map(|frame| f64::from(frame) * 1000.0 / f64::from(fps))
            .take_while(|&time| time < end)
            .collect();
        check(&times, &format!("{fps} FPS"));
    }
    // Повторы в момент тика L, на EOF и в группе с двумя итогами.
    check(
        &[
            10.0, 899.75, 900.0, 1280.0, 1500.0, 1500.25, 1500.25, 1510.0, 1510.0, 2000.0, 2000.25,
            2000.25, 2300.5, 2800.0,
        ],
        "нерегулярные и повторные времена",
    );
}
