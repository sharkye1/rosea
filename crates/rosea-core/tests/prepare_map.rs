use rosea_core::{
    error::RoseaError,
    model::{CurveKind, ObjectId, Point},
    parse::osu,
    prepare::{StackedObjectKind, prepare_map},
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
