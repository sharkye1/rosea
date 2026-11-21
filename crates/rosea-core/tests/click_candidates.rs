use rosea_core::{
    model::{ObjectId, TimeMs},
    parse::{osr, osu},
    prepare::prepare_map,
    replay::normalize,
    sim::{EventKind, Simulator},
};

fn events(objects: &str) -> Vec<(ObjectId, EventKind, Option<bool>)> {
    let text = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n{objects}"
    );
    let map = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    // LZMA-поток одной записи: два фронта в точке (0, 0) на 1000 мс.
    let compressed = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 48, 15, 145, 2, 51,
        23, 33, 242, 235, 255, 255, 220, 241, 0, 0,
    ];
    let mut bytes = vec![0];
    bytes.extend_from_slice(&20200427_i32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3 + 12 + 4 + 2 + 1 + 4 + 1 + 8]);
    bytes.extend_from_slice(&(compressed.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&compressed);
    let replay = normalize(osr(&bytes).unwrap().value.input).unwrap().value;
    let mut sim = Simulator::new(&map, &replay).unwrap();
    let mut events = Vec::new();
    sim.advance_with(TimeMs::new(1000.0).unwrap(), &mut |event| {
        events.push((event.object_id, event.kind, event.part_hit));
    })
    .unwrap();
    events
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
