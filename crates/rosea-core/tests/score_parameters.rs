use rosea_core::{error::RoseaError, parse::osu, prepare::prepare_map};

fn map(objects: &str, breaks: &str, difficulty: &str) -> String {
    format!(
        "osu file format v14\n[Difficulty]\nCircleSize:{difficulty}\nOverallDifficulty:{difficulty}\nHPDrainRate:{difficulty}\n[Events]\n{breaks}\n[HitObjects]\n{objects}"
    )
}

#[test]
fn drain_uses_object_starts_and_rounds_each_break_endpoint() {
    let text = map("0,0,0,1,0\n256,192,10000,8,0,20000", "2,2000,4000", "5");
    let prepared = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    assert_eq!(prepared.drain_seconds(), 8);

    let text = map("0,0,0.5,1,0\n0,0,2500.5,1,0", "", "5");
    let prepared = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    assert_eq!(prepared.drain_seconds(), 2);

    let text = map("0,0,0,1,0\n0,0,5000,1,0", "2,0.5,1500.5\n2,0.5,1500.5", "5");
    let prepared = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    assert_eq!(prepared.drain_seconds(), 2);
}

#[test]
fn computes_difficulty_with_zero_negative_and_even_ties() {
    let text = map("0,0,0,1,0", "", "5");
    let prepared = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    assert_eq!(prepared.drain_seconds(), 0);
    assert_eq!(prepared.difficulty_multiplier(), 4);

    let text = map("0,0,0,1,0\n0,0,1000,1,0", "2,0,3000", "5");
    let prepared = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    assert_eq!(prepared.drain_seconds(), -2);
    assert_eq!(prepared.difficulty_multiplier(), 2);

    let text = map("0,0,0,1,0\n0,0,4000,1,0", "", "5");
    let prepared = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    assert_eq!(prepared.drain_seconds(), 4);
    assert_eq!(prepared.difficulty_multiplier(), 2);

    let mut objects = String::new();
    for index in 0..100 {
        let time = if index == 99 { 100_000 } else { index * 1000 };
        objects.push_str(&format!("0,0,{time},1,0\n"));
    }
    let text = map(&objects, "", "5");
    let prepared = prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap();
    assert_eq!(prepared.drain_seconds(), 100);
    assert_eq!(prepared.difficulty_multiplier(), 3);
}

#[test]
fn rejects_break_times_outside_the_map_time_limit() {
    let text = map("0,0,0,1,0", "2,0,86400001", "5");
    assert!(matches!(
        prepare_map(&osu(text.as_bytes()).unwrap().value),
        Err(RoseaError::ResourceLimit {
            resource: "absolute_time_ms",
            limit: 86_400_000,
            observed: 86_400_001
        })
    ));
}
