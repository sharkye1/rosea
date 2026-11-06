//! Проверяем чтение L-слайдеров, общие ID с кругами и пределы исходных данных.

use rosea_core::error::RoseaError;
use rosea_core::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};
use rosea_core::parse::{osu_circles, osu_hit_objects};

fn map(objects: &str) -> String {
    format!("osu file format v14\n[HitObjects]\n{objects}")
}

#[test]
fn reads_mixed_objects_with_shared_ids_and_source_order_at_equal_times() {
    let text = "\u{feff}osu file format v14\r\n[HitObjects]\r\n\
        256,192,2000,6,2,L|300:192|300:250,2,102.5\r\n\r\n// comment\r\n\
        10,20,1000.25,1,0\r\n[Metadata]\r\nignored\r\n[HitObjects]\r\n\
         -5 , 400 , 1000.25 , 22 , 0 , L|0:410 , 1 , 12.5 \r\n";
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Ok(vec![
            HitObject {
                id: ObjectId(1),
                line: 6,
                position: Point { x: 10.0, y: 20.0 },
                start_time: 1000.25,
                flags: 1,
                hit_sound: 0,
                kind: HitObjectKind::Circle
            },
            HitObject {
                id: ObjectId(2),
                line: 10,
                position: Point { x: -5.0, y: 400.0 },
                start_time: 1000.25,
                flags: 22,
                hit_sound: 0,
                kind: HitObjectKind::Slider {
                    kind: CurveKind::Linear,
                    controls: vec![Point { x: -5.0, y: 400.0 }, Point { x: 0.0, y: 410.0 }],
                    spans: 1,
                    declared_length: 12.5
                }
            },
            HitObject {
                id: ObjectId(0),
                line: 3,
                position: Point { x: 256.0, y: 192.0 },
                start_time: 2000.0,
                flags: 6,
                hit_sound: 2,
                kind: HitObjectKind::Slider {
                    kind: CurveKind::Linear,
                    controls: vec![
                        Point { x: 256.0, y: 192.0 },
                        Point { x: 300.0, y: 192.0 },
                        Point { x: 300.0, y: 250.0 }
                    ],
                    spans: 2,
                    declared_length: 102.5
                }
            },
        ])
    );
}

#[test]
fn preserves_degenerate_controls_zero_length_and_pass_count() {
    // Совпадающие точки сохраняем: удаление дублей относится к построению пути.
    let text = map("10.5,-20.25,-0,2,0,L|10.5:-20.25|10.5:-20.25,10000,-0");
    let objects = osu_hit_objects(text.as_bytes()).unwrap();
    assert_eq!(objects[0].start_time.to_bits(), 0.0_f64.to_bits());
    let HitObjectKind::Slider {
        controls,
        spans,
        declared_length,
        ..
    } = &objects[0].kind
    else {
        panic!("ожидался слайдер");
    };
    assert_eq!(*controls, vec![Point { x: 10.5, y: -20.25 }; 3]);
    assert_eq!(*spans, 10_000);
    assert_eq!(declared_length.to_bits(), 0.0_f64.to_bits());
}

#[test]
fn rejects_missing_or_invalid_slider_fields_with_source_context() {
    let cases = [
        ("", "curveType"),
        ("L|300:200", "slides"),
        ("L|300:200,1", "length"),
        ("L,1,100", "curvePoints"),
        ("L|,1,100", "curvePoints"),
        ("L|300,1,100", "curvePoints"),
        ("L|300:200:100,1,100", "curvePoints"),
        ("L|NaN:200,1,100", "curvePoints"),
        ("L|300:inf,1,100", "curvePoints"),
        ("L|1e99:200,1,100", "curvePoints"),
        ("L|300:200,0,100", "slides"),
        ("L|300:200,-1,100", "slides"),
        ("L|300:200,1.5,100", "slides"),
        ("L|300:200,4294967296,100", "slides"),
        ("L|300:200,1,-1", "length"),
        ("L|300:200,1,NaN", "length"),
        ("L|300:200,1,inf", "length"),
        ("L|300:200,1,1e999", "length"),
    ];
    for (tail, field) in cases {
        let text = map(&format!("0,0,0,1,0\n256,192,1000,2,0,{tail}"));
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field,
                line: 4,
                object_id: ObjectId(1)
            }),
            "{tail}"
        );
    }
}

#[test]
fn rejects_other_curves_and_ambiguous_base_types() {
    for curve in ["B", "P", "C", "X"] {
        let text = map(&format!("256,192,1000,2,0,{curve}|300:200,1,100"));
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::UnsupportedHitObject {
                line: 3,
                object_id: ObjectId(0),
                flags: 2
            })
        );
    }
    for flags in [0, 4, 8, 128, 3, 9, 10, 130, 255] {
        let text = map(&format!("256,192,1000,{flags},0,L|300:200,1,100"));
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::UnsupportedHitObject {
                line: 3,
                object_id: ObjectId(0),
                flags
            })
        );
    }
}

#[test]
fn validates_optional_sound_metadata_without_opening_files() {
    for tail in ["", ",,,", ",2|1|2,0:0|0:0|0:2,0:0:0:0:missing,sound.wav"] {
        let text = map(&format!("256,192,1000,2,0,L|300:200,2,100{tail}"));
        assert!(osu_hit_objects(text.as_bytes()).is_ok(), "{tail}");
    }
    for (tail, field) in [
        (",abc", "edgeSounds"),
        (",256", "edgeSounds"),
        (",0|-1", "edgeSounds"),
        (",,0", "edgeSets"),
        (",,4:0", "edgeSets"),
        (",,0:0:0", "edgeSets"),
        (",,,0:0:abc:0:", "hitSample"),
    ] {
        let text = map(&format!("256,192,1000,2,0,L|300:200,1,100{tail}"));
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field,
                line: 3,
                object_id: ObjectId(0)
            })
        );
    }
}

#[test]
fn enforces_control_and_span_limits_before_growing_buffers() {
    // В лимит контрольных точек входит голова, поэтому в хвосте помещается 4095 точек.
    let curve = format!("L{}", "|1:1".repeat(4095));
    let text = map(&format!("0,0,0,2,0,{curve},1,100"));
    assert!(osu_hit_objects(text.as_bytes()).is_ok());
    let text = map(&format!("0,0,0,2,0,{curve}|1:1,1,100"));
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "slider_control_points",
            limit: 4096,
            observed: 4097
        })
    );
    let text = map("0,0,0,2,0,L|1:1,10001,100");
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "slider_spans",
            limit: 10000,
            observed: 10001
        })
    );
}

#[test]
fn enforces_a_shared_limit_for_mixed_objects() {
    let mut text = map(&"0,0,0,1,0\n0,0,0,2,0,L|1:1,1,1\n".repeat(50_000));
    let objects = osu_hit_objects(text.as_bytes()).unwrap();
    assert_eq!(objects.len(), 100_000);
    assert_eq!(objects[99_999].id, ObjectId(99_999));
    text.push_str("0,0,0,1,0");
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "hit_objects",
            limit: 100_000,
            observed: 100_001
        })
    );
}

#[test]
fn keeps_the_circle_only_entry_strict_and_checks_input_profile() {
    let text = map("256,192,1000,2,0,L|300:200,1,100");
    assert_eq!(
        osu_circles(text.as_bytes()),
        Err(RoseaError::UnsupportedHitObject {
            line: 3,
            object_id: ObjectId(0),
            flags: 2
        })
    );
    assert_eq!(osu_hit_objects(b"\xff"), Err(RoseaError::InvalidEncoding));
    assert_eq!(
        osu_hit_objects(b"osu file format v13"),
        Err(RoseaError::UnsupportedVersion(13))
    );
    assert_eq!(
        osu_hit_objects(map("").as_bytes()),
        Err(RoseaError::EmptyBeatmap)
    );
    let text = "osu file format v14\n[General]\nMode: 1\n[HitObjects]\n0,0,0,2,0,L|1:1,1,1";
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Err(RoseaError::UnsupportedMode(1))
    );
}
