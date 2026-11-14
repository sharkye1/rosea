use rosea_core::{parse::osu, prepare::visibility};

fn map(cs: f32, ar: f32, objects: &str) -> rosea_core::model::Beatmap {
    let text = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:{cs}\nOverallDifficulty:5\nHPDrainRate:5\nApproachRate:{ar}\nSliderMultiplier:1\nSliderTickRate:1\n[HitObjects]\n{objects}"
    );
    osu(text.as_bytes()).unwrap().value
}

#[test]
fn computes_radius_and_visibility_from_difficulty() {
    for (cs, expected) in [
        (0.0, 54.422_302_246_1),
        (5.0, 32.013_118_743_9),
        (10.0, 9.603_936_195_4),
    ] {
        let beatmap = map(cs, 5.0, "256,192,2000,1,0");
        let prepared = visibility(&beatmap).unwrap();
        assert!((f64::from(prepared.radius) - expected).abs() <= 1e-8);
        assert_eq!(prepared.preempt, 1200.0);
        assert_eq!(prepared.fade_in, 400.0);
        assert_eq!(prepared.first_visible_at.get(), 800.0);
    }
    for (ar, preempt) in [
        (0.0, 1800.0),
        (5.0, 1200.0),
        (10.0, 450.0),
        (4.99, 1201.0),
        (5.01, 1198.0),
    ] {
        let beatmap = map(5.0, ar, "256,192,2000.25,1,0");
        let prepared = visibility(&beatmap).unwrap();
        assert_eq!(prepared.preempt, preempt);
        assert_eq!(prepared.fade_in, 400.0);
        assert_eq!(prepared.first_visible_at.get(), 2000.25 - preempt);
    }
}

#[test]
fn first_visible_accounts_for_spinner_and_preempt() {
    let beatmap = map(5.0, 5.0, "256,192,200,8,0,1000\n256,192,1000.25,1,0");
    assert_eq!(
        visibility(&beatmap).unwrap().first_visible_at.get(),
        -199.75
    );

    let beatmap = map(5.0, 5.0, "256,192,100,8,0,1000\n256,192,2000,1,0");
    assert_eq!(visibility(&beatmap).unwrap().first_visible_at.get(), 100.0);

    let beatmap = map(5.0, 5.0, "256,192,0,8,0,1000");
    assert_eq!(visibility(&beatmap).unwrap().first_visible_at.get(), 0.0);

    let beatmap = map(5.0, 5.0, "256,192,1000.25,2,0,L|300:192,1,44");
    assert_eq!(
        visibility(&beatmap).unwrap().first_visible_at.get(),
        -199.75
    );
}
