use rosea_core::{
    model::{Judgement, TimeMs},
    parse::osu,
    prepare::prepare_map,
    sim::hit::judge_click,
};

fn windows(od: &str) -> rosea_core::prepare::HitWindows {
    let text = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:{od}\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0"
    );
    prepare_map(&osu(text.as_bytes()).unwrap().value)
        .unwrap()
        .hit_windows()
}

#[test]
fn judges_both_sides_of_each_strict_window() {
    let windows = windows("8");
    let start = TimeMs::new(1000.0).unwrap();
    for (offset, expected) in [
        (0, Some(Judgement::Great)),
        (31, Some(Judgement::Great)),
        (32, Some(Judgement::Ok)),
        (75, Some(Judgement::Ok)),
        (76, Some(Judgement::Meh)),
        (119, Some(Judgement::Meh)),
        (120, None),
        (121, None),
    ] {
        for click in [1000 - offset, 1000 + offset] {
            assert_eq!(
                judge_click(windows, start, click),
                expected,
                "click={click}"
            );
        }
    }
}

#[test]
fn preserves_fractional_start_and_od() {
    let integral = windows("8");
    assert_eq!(
        judge_click(integral, TimeMs::new(1000.25).unwrap(), 1032),
        Some(Judgement::Great)
    );
    assert_eq!(
        judge_click(integral, TimeMs::new(1000.0).unwrap(), 1032),
        Some(Judgement::Ok)
    );

    let fractional = windows("8.8");
    let start = TimeMs::new(1000.0).unwrap();
    assert_eq!(judge_click(fractional, start, 1026), Some(Judgement::Great));
    assert_eq!(judge_click(fractional, start, 1027), Some(Judgement::Ok));
    assert_eq!(judge_click(fractional, start, 1069), Some(Judgement::Meh));
    assert_eq!(judge_click(fractional, start, 1110), Some(Judgement::Meh));
    assert_eq!(judge_click(fractional, start, 1111), None);
}
