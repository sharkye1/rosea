use rosea_core::{
    error::RoseaError,
    model::{CurveKind, ObjectId, Point},
    parse::osu,
    prepare::{HitWindows, StackedObjectKind, prepare_map},
};

#[test]
fn prepares_mixed_map_with_owned_paths_and_original_ids() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:5\nHPDrainRate:5\nApproachRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n256,192,5000,8,0,6000\n100,100,1100,1,0\n100,100,1000,1,0\n0,0,2000,2,0,L|100:0,1,100\n0,100,3000,2,0,B|50:200|100:100,1,100\n100,100,4000,2,0,P|150:150|200:100,1,100";
    let beatmap = osu(input).unwrap().value;
    let prepared = prepare_map(&beatmap).unwrap();
    drop(beatmap);

    assert_eq!(prepared.first_visible_at().get(), -200.0);
    assert_eq!(prepared.visibility().preempt, 1200.0);
    let objects = prepared.objects();
    assert_eq!(
        objects.iter().map(|o| o.object_id().0).collect::<Vec<_>>(),
        vec![2, 1, 3, 4, 5, 0]
    );
    assert_eq!(objects[0].kind(), StackedObjectKind::Circle);
    assert_eq!(objects[0].start_time(), 1000.0);
    assert_eq!(objects[0].height(), 1);
    assert_ne!(objects[0].position(), Point { x: 100.0, y: 100.0 });
    assert_eq!(objects[1].height(), 0);
    for (object, kind) in objects[2..5].iter().zip([
        CurveKind::Linear,
        CurveKind::Bezier,
        CurveKind::PerfectCurve,
    ]) {
        assert_eq!(object.kind(), StackedObjectKind::Slider(kind));
        let slider = object.slider().unwrap();
        assert_eq!(slider.timing().object_id, object.object_id());
        assert_eq!(slider.path().length(), 100.0);
    }
    let slider = objects[2].slider().unwrap();
    let original_path = slider.path() as *const _;
    assert_eq!(
        original_path,
        prepared.objects()[2].slider().unwrap().path() as *const _
    );
    assert_eq!(slider.timing().start_time, 2000.0);
    assert_eq!(slider.timing().end_time, 2500.0);
    assert_eq!(
        objects[2].position_at_time(2250.0).unwrap(),
        Point { x: 50.0, y: 0.0 }
    );
    assert_eq!(
        objects[5].kind(),
        StackedObjectKind::Spinner { end: 6000.0 }
    );
    assert!(objects[5].slider().is_none());
}

#[test]
fn reports_missing_bpm_from_preparation_with_object_context() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:5\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,100";
    let beatmap = osu(input).unwrap().value;
    assert!(matches!(
        prepare_map(&beatmap),
        Err(RoseaError::PreprocessError {
            object_id: ObjectId(0),
            ..
        })
    ));
}

#[test]
fn prepares_windows_and_deadlines_for_fractional_and_zero_duration_objects() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8.8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000.25,1,0\n0,100,2000.25,2,0,L|100:100,1,0\n0,200,3000,2,0,L|100:200,1,100\n256,192,4000,8,0,4500";
    let prepared = prepare_map(&osu(input).unwrap().value).unwrap();
    assert_eq!(
        prepared.hit_windows(),
        HitWindows {
            great: 27,
            ok: 69,
            meh: 111
        }
    );
    let times = prepared.object_times();
    assert_eq!(times[0].start.get(), 1000.25);
    assert_eq!(times[0].end.get(), 1000.25);
    assert_eq!(times[0].head_deadline.unwrap().get(), 1111.25);
    assert_eq!(times[1].end.get(), 2000.25);
    assert_eq!(times[1].latest_resolution.get(), 2111.25);
    assert_eq!(times[2].end.get(), 3500.0);
    assert_eq!(times[2].head_deadline.unwrap().get(), 3111.0);
    assert_eq!(times[2].latest_resolution.get(), 3500.0);
    assert_eq!(times[3].end.get(), 4500.0);
    assert_eq!(times[3].head_deadline, None);
    assert_eq!(times[3].latest_resolution.get(), 4500.0);

    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0";
    assert_eq!(
        prepare_map(&osu(input).unwrap().value)
            .unwrap()
            .hit_windows(),
        HitWindows {
            great: 32,
            ok: 76,
            meh: 120
        }
    );
}

#[test]
fn rejects_deadlines_beyond_time_limit() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,86399900,1,0";
    assert!(matches!(
        prepare_map(&osu(input).unwrap().value),
        Err(RoseaError::ResourceLimit {
            resource: "absolute_time_ms",
            limit: 86_400_000,
            observed: 86_400_020
        })
    ));
}

#[test]
fn enforces_shared_slider_event_budget_across_the_map() {
    let mut input = String::from(
        "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:5\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n",
    );
    for _ in 0..201 {
        input.push_str("0,0,1000,2,0,L|100:0,10000,0\n");
    }
    let beatmap = osu(input.as_bytes()).unwrap().value;
    assert!(matches!(
        prepare_map(&beatmap),
        Err(RoseaError::ResourceLimit {
            resource: "slider_events",
            limit: 2_000_000,
            observed: 2_000_001
        })
    ));
}
