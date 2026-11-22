use rosea_core::{
    model::{ObjectId, TimeMs},
    parse::{osr, osu},
    prepare::{PreparedMap, prepare_map},
    replay::{ReplayTimeline, normalize},
    sim::{
        EventKind, Simulator,
        input::{InputAction, InputEvent},
        timeline::{Timeline, TimelineEvent},
    },
};

fn map(objects: &str) -> PreparedMap {
    let text = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n{objects}"
    );
    prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap()
}

fn replay(compressed: &[u8]) -> ReplayTimeline {
    let mut bytes = vec![0];
    bytes.extend_from_slice(&20200427_i32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3 + 12 + 4 + 2 + 1 + 4 + 1 + 8]);
    bytes.extend_from_slice(&(compressed.len() as i32).to_le_bytes());
    bytes.extend_from_slice(compressed);
    normalize(osr(&bytes).unwrap().value.input).unwrap().value
}

fn events(objects: &str) -> Vec<(ObjectId, EventKind, Option<bool>)> {
    let map = map(objects);
    // LZMA-поток одной записи: два фронта в точке (0, 0) на 1000 мс.
    let compressed = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 48, 15, 145, 2, 51,
        23, 33, 242, 235, 255, 255, 220, 241, 0, 0,
    ];
    let replay = replay(&compressed);
    let mut sim = Simulator::new(&map, &replay).unwrap();
    let mut events = Vec::new();
    sim.advance_with(TimeMs::new(1000.0).unwrap(), &mut |event| {
        events.push((event.object_id, event.kind, event.part_hit));
    })
    .unwrap();
    events
}

#[test]
fn overlapping_sliders_keep_heads_and_parts_independent() {
    let map = map("0,0,1000,2,0,L|200:0,1,200\n300,0,1000,2,0,L|500:0,2,200");
    // Две головы нажаты раздельно; затем курсор следует только за вторым шариком.
    let compressed = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 48, 15, 145, 2, 49,
        22, 235, 172, 31, 203, 156, 103, 213, 255, 96, 2, 147, 36, 10, 172, 111, 67, 92, 20, 242,
        10, 98, 235, 150, 113, 190, 237, 220, 46, 133, 247, 58, 7, 53, 255, 255, 217, 99, 128, 0,
    ];
    let replay = replay(&compressed);
    let mut sim = Simulator::new(&map, &replay).unwrap();
    let mut observed = Vec::new();
    sim.advance_with(TimeMs::new(3000.0).unwrap(), &mut |event| {
        observed.push((
            event.time.get(),
            event.object_id,
            event.kind,
            event.part_hit,
        ));
    })
    .unwrap();
    assert_eq!(
        observed,
        [
            (1000.0, ObjectId(0), EventKind::SliderHead, Some(true)),
            (1100.0, ObjectId(1), EventKind::SliderHead, Some(true)),
            (1500.0, ObjectId(0), EventKind::SliderTick, Some(false)),
            (1500.0, ObjectId(1), EventKind::SliderTick, Some(true)),
            (1964.0, ObjectId(0), EventKind::SliderTail, Some(false)),
            (2000.0, ObjectId(1), EventKind::SliderRepeat, Some(true)),
            (2500.0, ObjectId(1), EventKind::SliderTick, Some(false)),
            (2964.0, ObjectId(1), EventKind::SliderTail, Some(false)),
        ]
    );
}

#[test]
fn simultaneous_fronts_reselect_and_held_actions_do_not_click() {
    let map =
        map("0,0,1000,1,0\n0,0,1000,2,0,L|200:0,1,200\n0,0,1010,1,0\n0,0,1020,2,0,L|200:0,1,200");
    // Записи: 1000:15, 1000:15, 1010:15, 1020:10, 1030:15,
    // 1040:5, 1050:15, 1060:15; курсор всегда в (0, 0).
    let compressed = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 48, 15, 145, 2, 49,
        28, 1, 188, 116, 66, 171, 152, 138, 162, 229, 254, 96, 244, 182, 137, 19, 127, 255, 255, 6,
        8, 0, 0,
    ];
    let replay = replay(&compressed);
    let mut timeline = Timeline::new(&map, &replay);
    let mut presses = Vec::new();
    timeline
        .advance_to(TimeMs::new(1060.0).unwrap(), |time, event| {
            if let TimelineEvent::Input(InputEvent::Press { action, .. }) = event {
                presses.push((time.get(), action));
            }
        })
        .unwrap();
    assert_eq!(
        presses,
        [
            (1000.0, InputAction::Left),
            (1000.0, InputAction::Right),
            (1030.0, InputAction::Left),
            (1050.0, InputAction::Right),
        ]
    );

    let mut sim = Simulator::new(&map, &replay).unwrap();
    let mut observed = Vec::new();
    for time in [1000.0, 1010.0, 1020.0, 1030.0, 1040.0, 1050.0, 1060.0] {
        sim.advance_with(TimeMs::new(time).unwrap(), &mut |event| {
            observed.push((
                event.time.get(),
                event.object_id,
                event.kind,
                event.part_hit,
            ));
        })
        .unwrap();
        let count = if time < 1030.0 {
            2
        } else if time < 1050.0 {
            3
        } else {
            4
        };
        assert_eq!(observed.len(), count, "{time} мс");
    }
    assert_eq!(
        observed,
        [
            (1000.0, ObjectId(0), EventKind::Circle, None),
            (1000.0, ObjectId(1), EventKind::SliderHead, Some(true)),
            (1030.0, ObjectId(2), EventKind::Circle, None),
            (1050.0, ObjectId(3), EventKind::SliderHead, Some(true)),
        ]
    );
    assert_eq!(sim.stats().current_combo, 4);
}

#[test]
fn mixed_candidates_skip_out_of_radius_and_choose_equal_starts_by_id() {
    for (objects, expected) in [
        (
            "0,0,1000,1,0\n0,0,1000,2,0,L|200:0,1,200\n200,0,900,2,0,L|400:0,1,200",
            [
                (ObjectId(0), EventKind::Circle, None),
                (ObjectId(1), EventKind::SliderHead, Some(true)),
            ],
        ),
        (
            "0,0,1000,2,0,L|200:0,1,200\n0,0,1000,1,0\n200,0,900,1,0",
            [
                (ObjectId(0), EventKind::SliderHead, Some(true)),
                (ObjectId(1), EventKind::Circle, None),
            ],
        ),
    ] {
        assert_eq!(events(objects), expected);
    }
}

#[test]
fn strict_w50_skips_expired_head_and_future_circle() {
    let objects = "0,0,880,2,0,L|200:0,1,200\n0,0,1120,1,0\n0,0,1000,1,0";
    assert_eq!(
        events(objects),
        [
            (ObjectId(2), EventKind::Circle, None),
            (ObjectId(0), EventKind::SliderHead, Some(false)),
        ]
    );
}
