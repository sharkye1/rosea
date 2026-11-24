use rosea_core::{
    model::{HitCounts, Judgement, ObjectId, ParsedReplay, TimeMs},
    parse::{osr, osu},
    prepare::{PreparedMap, prepare_map},
    replay::normalize,
    sim::{EventKind, Simulator},
};
use std::fmt::Write as _;

fn map(objects: &str) -> PreparedMap {
    let text = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n{objects}"
    );
    prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap()
}

fn replay(records: &str, game_version: i32) -> ParsedReplay {
    let mut compressed = Vec::new();
    lzma_rs::lzma_compress(&mut records.as_bytes(), &mut compressed).unwrap();
    let mut bytes = vec![0];
    bytes.extend_from_slice(&game_version.to_le_bytes());
    bytes.extend_from_slice(&[0; 3 + 12 + 4 + 2 + 1 + 4 + 1 + 8]);
    bytes.extend_from_slice(&i32::try_from(compressed.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&compressed);
    osr(&bytes).unwrap().value
}

fn alternating_positions(halves: u32, step_ms: u32) -> String {
    let mut records = String::from("1000|356|192|1,");
    for half in 1..=halves {
        let x = if half.is_multiple_of(2) { 356 } else { 156 };
        write!(records, "{step_ms}|{x}|192|1,").unwrap();
    }
    records
}

#[test]
fn overlapping_spinners_follow_object_ids_and_group_phases() {
    let map = map(concat!(
        "256,192,1010,8,0,1100\n",
        "256,192,1000,8,0,1100\n",
        "256,192,1100,8,0,1100\n",
        "156,192,1100,1,0",
    ));
    let replay = replay(
        concat!(
            "1000|356|192|1,10|356|192|1,20|156|192|1,20|356|192|1,",
            "20|156|192|1,20|356|192|1,10|156|192|3,10|356|192|3,",
        ),
        20200427,
    );
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

#[test]
fn normative_spinner_scores_through_public_simulator() {
    use EventKind::{SpinnerBonus, SpinnerPoints, SpinnerResult};

    // Полуобороты, на которых спецификация начисляет очки. В чётном пороге
    // бонус замещает обычное начисление, в нечётном обе серии чередуются.
    type Case = (u32, u32, Judgement, u64, &'static [u32], &'static [u32]);
    let cases: [Case; 7] = [
        (100, 0, Judgement::Great, 300, &[], &[]),
        (2000, 0, Judgement::Miss, 0, &[], &[]),
        (2000, 3, Judgement::Meh, 150, &[2], &[]),
        (2000, 12, Judgement::Ok, 700, &[2, 4, 6, 8, 10, 12], &[]),
        (
            2000,
            14,
            Judgement::Great,
            1000,
            &[2, 4, 6, 8, 10, 12, 14],
            &[],
        ),
        (
            2000,
            20,
            Judgement::Great,
            3300,
            &[2, 4, 6, 8, 10, 12, 14, 16],
            &[18, 20],
        ),
        (
            1996,
            24,
            Judgement::Great,
            5900,
            &[2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24],
            &[17, 19, 21, 23],
        ),
    ];
    for (duration, halves, judgement, score, points, bonuses) in cases {
        let end = 1000 + duration;
        let map = map(&format!("256,192,1000,8,0,{end}"));
        let input = normalize(replay(&alternating_positions(halves, 50), 20200427).input)
            .unwrap()
            .value;
        let mut events = Vec::new();
        let stats = Simulator::new(&map, &input)
            .unwrap()
            .finish_with(&mut |event| events.push(*event))
            .unwrap();
        let mut expected = Vec::new();
        expected.extend(
            points
                .iter()
                .map(|half| (f64::from(1000 + half * 50), SpinnerPoints, 100)),
        );
        expected.extend(
            bonuses
                .iter()
                .map(|half| (f64::from(1000 + half * 50), SpinnerBonus, 1100)),
        );
        expected.sort_by(|left, right| left.0.total_cmp(&right.0));
        let observed: Vec<_> = events[..events.len() - 1]
            .iter()
            .map(|event| (event.time.get(), event.kind, event.score_delta))
            .collect();
        assert_eq!(observed, expected, "duration={duration}, H={halves}");
        let result = events.last().unwrap();
        let combo = u32::from(judgement != Judgement::Miss);
        assert_eq!(
            (result.time.get(), result.kind, result.judgement),
            (f64::from(end), SpinnerResult, Some(judgement))
        );
        assert_eq!((result.combo_before, result.combo_after), (0, combo));
        let expected_counts = match judgement {
            Judgement::Great => HitCounts {
                count_300: 1,
                ..HitCounts::default()
            },
            Judgement::Ok => HitCounts {
                count_100: 1,
                ..HitCounts::default()
            },
            Judgement::Meh => HitCounts {
                count_50: 1,
                ..HitCounts::default()
            },
            Judgement::Miss => HitCounts {
                count_miss: 1,
                ..HitCounts::default()
            },
        };
        assert_eq!(stats.counts, expected_counts);
        assert_eq!(
            (stats.score, stats.current_combo, stats.max_combo),
            (score, combo, combo)
        );
    }
}

#[test]
fn spinner_geometry_produces_same_results_across_versions_and_frame_rates() {
    let map = map("256,192,1000,8,0,3000");
    // Одинаковые полуобороты занимают 1 или 90 мс каждый. Первая серия
    // превышает 477 RPM уже на первом интервале, вторая укладывается в предел.
    let fast = alternating_positions(20, 1);
    let slow = alternating_positions(20, 90);
    let baseline_input = normalize(replay(&fast, 20200427).input).unwrap().value;
    let mut baseline_journal = Vec::new();
    let baseline = Simulator::new(&map, &baseline_input)
        .unwrap()
        .finish_with(&mut |event| baseline_journal.push(*event))
        .unwrap();
    assert_eq!(baseline.score, 3300);
    let slow_input = normalize(replay(&slow, 20200427).input).unwrap().value;
    assert_eq!(
        Simulator::new(&map, &slow_input).unwrap().finish().unwrap(),
        baseline
    );

    for version in [20180101, 20200427, 20261124] {
        let parsed = replay(&fast, version);
        assert_eq!(parsed.meta.game_version(), version);
        let input = normalize(parsed.input).unwrap().value;
        for fps in [30.0, 60.0, 144.0] {
            let mut sim = Simulator::new(&map, &input).unwrap();
            let mut journal = Vec::new();
            let frames = (0..)
                .map(|frame| f64::from(frame) * 1000.0 / fps)
                .take_while(|&time| time < 3000.0);
            for time in frames {
                sim.advance_with(TimeMs::new(time).unwrap(), &mut |event| {
                    journal.push(*event)
                })
                .unwrap();
            }
            let stats = sim.finish_with(&mut |event| journal.push(*event)).unwrap();
            assert_eq!(stats, baseline, "game_version={version}, FPS={fps}");
            assert_eq!(
                journal, baseline_journal,
                "game_version={version}, FPS={fps}"
            );
        }
    }
}
