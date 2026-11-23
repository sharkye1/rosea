use rosea_core::{
    model::ObjectId,
    parse::osu,
    prepare::{SpinnerParameters, prepare_map},
};

fn prepared(od: &str, objects: &str) -> rosea_core::prepare::PreparedMap {
    let text = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:{od}\nHPDrainRate:5\n[HitObjects]\n{objects}"
    );
    prepare_map(&osu(text.as_bytes()).unwrap().value).unwrap()
}

#[test]
fn prepares_spinner_thresholds_at_od_boundaries_and_short_durations() {
    for (od, duration, req_half) in [
        ("0", "2000", 6),
        ("4.5", "2000", 9),
        ("5", "2000", 10),
        ("8", "2000", 13),
        ("10", "2000", 15),
        ("8", "100", 0),
        ("8", "1996", 12),
        ("8", "0", 0),
    ] {
        let end: f64 = 1000.25 + duration.parse::<f64>().unwrap();
        let map = prepared(od, &format!("256,192,1000.25,8,0,{end}"));
        assert_eq!(
            map.spinner_parameters(ObjectId(0)),
            Some(SpinnerParameters {
                duration_ms: duration.parse().unwrap(),
                req_half,
                req_bonus: req_half + 3,
            }),
            "OD={od}, duration={duration}"
        );
    }
}

#[test]
fn uses_source_ids_after_sorting_and_returns_none_for_non_spinners() {
    let map = prepared(
        "8",
        "0,0,5000,1,0\n256,192,1000,8,0,3000\n256,192,2000,8,0,2100",
    );
    assert_eq!(map.spinner_parameters(ObjectId(0)), None);
    assert_eq!(
        map.spinner_parameters(ObjectId(1)),
        Some(SpinnerParameters {
            duration_ms: 2000.0,
            req_half: 13,
            req_bonus: 16
        })
    );
    assert_eq!(
        map.spinner_parameters(ObjectId(2)),
        Some(SpinnerParameters {
            duration_ms: 100.0,
            req_half: 0,
            req_bonus: 3
        })
    );
    assert_eq!(map.spinner_parameters(ObjectId(u32::MAX)), None);
}
