use rosea_core::{
    error::RoseaError,
    model::{HitCounts, Judgement, Stats},
    sim::score::{apply_circle, award},
};

#[test]
fn calculates_points_from_combo_before_the_result() {
    assert_eq!(award(Judgement::Great, 0, 5), Ok(300));
    assert_eq!(award(Judgement::Great, 1, 5), Ok(300));
    assert_eq!(award(Judgement::Great, 2, 5), Ok(360));
    assert_eq!(award(Judgement::Ok, 3, 5), Ok(140));
    assert_eq!(award(Judgement::Meh, 3, 5), Ok(70));
    assert_eq!(award(Judgement::Miss, u32::MAX, u32::MAX), Ok(0));
    assert_eq!(
        award(Judgement::Great, u32::MAX, u32::MAX),
        Err(RoseaError::ScoreOverflow)
    );
}

#[test]
fn updates_combo_maximum_score_and_counts_for_circles() {
    let mut stats = Stats::default();
    let mut awarded = Vec::new();
    for result in [
        Judgement::Great,
        Judgement::Great,
        Judgement::Great,
        Judgement::Miss,
        Judgement::Ok,
    ] {
        awarded.push(apply_circle(&mut stats, result, 5).unwrap());
    }
    assert_eq!(awarded, [300, 300, 360, 0, 100]);
    assert_eq!(
        stats,
        Stats {
            score: 1060,
            current_combo: 1,
            max_combo: 3,
            counts: HitCounts {
                count_300: 3,
                count_100: 1,
                count_50: 0,
                count_miss: 1
            },
        }
    );
    assert_eq!(apply_circle(&mut stats, Judgement::Meh, 5), Ok(50));
    assert_eq!(stats.counts.count_50, 1);
    assert_eq!(stats.current_combo, 2);
    assert_eq!(stats.max_combo, 3);
}

#[test]
fn overflow_does_not_partially_update_statistics() {
    let cases = [
        (
            Stats {
                score: u64::MAX,
                ..Stats::default()
            },
            Judgement::Great,
            5,
        ),
        (
            Stats {
                current_combo: u32::MAX,
                max_combo: u32::MAX,
                ..Stats::default()
            },
            Judgement::Great,
            u32::MAX,
        ),
        (
            Stats {
                current_combo: u32::MAX,
                max_combo: u32::MAX,
                ..Stats::default()
            },
            Judgement::Great,
            0,
        ),
        (
            Stats {
                counts: HitCounts {
                    count_miss: u32::MAX,
                    ..HitCounts::default()
                },
                ..Stats::default()
            },
            Judgement::Miss,
            5,
        ),
    ];
    for (mut stats, result, difficulty) in cases {
        let original = stats;
        assert_eq!(
            apply_circle(&mut stats, result, difficulty),
            Err(RoseaError::ScoreOverflow)
        );
        assert_eq!(stats, original);
    }
}
