use rosea_core::{
    model::{HitCounts, Judgement, ObjectId, ParsedReplay, TimeMs},
    parse::{osr, osu},
    prepare::{PreparedMap, prepare_map},
    replay::normalize,
    sim::{EventKind, Simulator},
};

fn map(objects: &str) -> PreparedMap {
    let text = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n{objects}"
    );
    prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap()
}

fn replay(records: &str) -> ParsedReplay {
    let mut compressed = Vec::new();
    lzma_rs::lzma_compress(&mut records.as_bytes(), &mut compressed).unwrap();
    let mut bytes = vec![0];
    bytes.extend_from_slice(&20200427_i32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3 + 12 + 4 + 2 + 1 + 4 + 1 + 8]);
    bytes.extend_from_slice(&i32::try_from(compressed.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&compressed);
    osr(&bytes).unwrap().value
}

#[test]
fn overlapping_spinners_follow_object_ids_and_group_phases() {
    let map = map(concat!(
        "256,192,1010,8,0,1100\n",
        "256,192,1000,8,0,1100\n",
        "256,192,1100,8,0,1100\n",
        "156,192,1100,1,0",
    ));
    let replay = replay(concat!(
        "1000|356|192|1,10|356|192|1,20|156|192|1,20|356|192|1,",
        "20|156|192|1,20|356|192|1,10|156|192|3,10|356|192|3,",
    ));
    let input = normalize(replay.input).unwrap().value;
    let mut sim = Simulator::new(&map, &input).unwrap();
    let mut events = Vec::new();
    for time in [1049.0, 1050.0, 1099.0, 1100.0, 1100.0] {
        sim.advance_with(TimeMs::new(time).unwrap(), &mut |event| events.push(*event))
            .unwrap();
    }
    let stats = sim.finish_with(&mut |event| events.push(*event)).unwrap();

    use EventKind::{Circle, SpinnerBonus, SpinnerPoints, SpinnerResult};
    let observed: Vec<_> = events
        .iter()
        .map(|event| (event.time.get(), event.object_id, event.kind))
        .collect();
    assert_eq!(
        observed,
        [
            (1050.0, ObjectId(0), SpinnerPoints),
            (1050.0, ObjectId(1), SpinnerPoints),
            (1090.0, ObjectId(0), SpinnerPoints),
            (1090.0, ObjectId(1), SpinnerPoints),
            (1100.0, ObjectId(0), SpinnerBonus),
            (1100.0, ObjectId(1), SpinnerBonus),
            (1100.0, ObjectId(3), Circle),
            (1100.0, ObjectId(0), SpinnerResult),
            (1100.0, ObjectId(1), SpinnerResult),
            (1100.0, ObjectId(2), SpinnerResult),
        ],
    );
    for event in &events[..6] {
        assert_eq!(
            (event.judgement, event.combo_before, event.combo_after),
            (None, 0, 0)
        );
    }
    for (combo, event) in (0..).zip(&events[6..]) {
        assert_eq!(
            (event.judgement, event.combo_before, event.combo_after),
            (Some(Judgement::Great), combo, combo + 1)
        );
    }
    assert_eq!(
        stats.counts,
        HitCounts {
            count_300: 4,
            ..HitCounts::default()
        }
    );
    assert_eq!(
        stats.score,
        3800 + 36 * u64::from(map.difficulty_multiplier())
    );
    assert_eq!(
        Simulator::new(&map, &input).unwrap().finish().unwrap(),
        stats
    );
}
