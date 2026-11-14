use rosea_core::{
    model::ObjectId,
    parse::osu,
    prepare::{MapEventKind, prepare_map},
};

#[test]
fn orders_simultaneous_events_by_phase_kind_and_object() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,2,0\n100,100,1000,2,0,L|200:100,2,0\n200,100,880,1,0\n256,192,1000,8,0,1000\n300,100,880.000001,1,0";
    let prepared = prepare_map(&osu(input).unwrap().value).unwrap();
    let events = prepared.events();
    let same_time: Vec<_> = events
        .iter()
        .filter(|event| event.time.get() == 1000.0)
        .map(|event| (event.kind, event.object_id.0, event.part_index))
        .collect();
    use MapEventKind::*;
    assert_eq!(
        same_time,
        [
            (SpinnerStart, 3, None),
            (SliderStart, 0, None),
            (SliderStart, 1, None),
            (SliderRepeat, 0, Some(0)),
            (SliderRepeat, 1, Some(0)),
            (SliderTailCheck, 0, Some(1)),
            (SliderTailCheck, 1, Some(1)),
            (SliderEnd, 0, None),
            (SliderEnd, 1, None),
            (CircleDeadline, 2, None),
            (SpinnerEnd, 3, None),
        ]
    );
    let later = events
        .iter()
        .find(|event| event.object_id == ObjectId(4))
        .unwrap();
    assert_eq!(later.kind, CircleDeadline);
    assert_eq!(later.time.get(), 1000.000001);
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].time.get() <= pair[1].time.get())
    );
    let late_heads: Vec<_> = events
        .iter()
        .filter(|event| event.kind == SliderHeadDeadline)
        .collect();
    assert_eq!(late_heads.len(), 2);
    assert!(late_heads.iter().all(|event| event.time.get() == 1120.0));
}

#[test]
fn keeps_fractional_part_times_and_original_indices() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:5\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:2\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000.25,2,0,L|100:0,1,100";
    let prepared = prepare_map(&osu(input).unwrap().value).unwrap();
    let slider = prepared.objects()[0].slider().unwrap();
    let parts: Vec<_> = prepared
        .events()
        .iter()
        .filter_map(|event| {
            event
                .part_index
                .map(|index| (event.time.get(), index, event.kind))
        })
        .collect();
    assert!(!parts.is_empty());
    for (time, index, _) in parts {
        assert_eq!(time, slider.timing().events[index as usize].time);
        assert_eq!(time.fract(), 0.25);
    }
}

#[test]
fn closes_slider_head_before_its_end_at_the_same_time() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|24:0,1,24";
    let prepared = prepare_map(&osu(input).unwrap().value).unwrap();
    let kinds: Vec<_> = prepared
        .events()
        .iter()
        .filter(|event| event.time.get() == 1120.0)
        .map(|event| event.kind)
        .collect();
    assert_eq!(
        kinds,
        [MapEventKind::SliderHeadDeadline, MapEventKind::SliderEnd]
    );
}
