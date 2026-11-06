use rosea_core::error::RoseaError;
use rosea_core::model::{Circle, ObjectId, Point};
use rosea_core::parse::osu_circles;

fn map(objects: &str) -> String {
    format!("osu file format v14\n[HitObjects]\n{objects}")
}

#[test]
fn reads_all_circles_and_preserves_source_ids_after_sorting() {
    let text = "\u{feff}osu file format v14\r\n\
        [Metadata]\r\nignored\r\n[HitObjects]\r\n\
        256,192,2000,21,2\r\n\r\n// comment\r\n\
        -20,420,1000.25,1,0,0:0:0:0:\r\n\
        [Metadata]\r\nignored\r\n[HitObjects]\r\n\
         512 , 384 , 1000.25 , 117 , 15 , 0:0:0:0:sound,with,commas.wav\r\n";
    assert_eq!(
        osu_circles(text.as_bytes()),
        Ok(vec![
            Circle {
                id: ObjectId(1),
                line: 8,
                position: Point { x: -20.0, y: 420.0 },
                start_time: 1000.25,
                flags: 1,
                hit_sound: 0
            },
            Circle {
                id: ObjectId(2),
                line: 12,
                position: Point { x: 512.0, y: 384.0 },
                start_time: 1000.25,
                flags: 117,
                hit_sound: 15
            },
            Circle {
                id: ObjectId(0),
                line: 5,
                position: Point { x: 256.0, y: 192.0 },
                start_time: 2000.0,
                flags: 21,
                hit_sound: 2
            },
        ])
    );
}

#[test]
fn preserves_fractional_positions_and_negative_time_and_canonicalizes_zero() {
    let circles = osu_circles(map("10.5,-20.25,-0,1,0\n0,0,-1.25,1,0,").as_bytes()).unwrap();
    assert_eq!(circles[0].start_time, -1.25);
    assert_eq!(circles[1].position, Point { x: 10.5, y: -20.25 });
    assert_eq!(circles[1].start_time.to_bits(), 0.0_f64.to_bits());
}

#[test]
fn rejects_empty_hit_objects() {
    for body in [
        "",
        "[HitObjects]\n\n// comment",
        "[Metadata]\n256,192,1000,1,0",
    ] {
        let text = format!("osu file format v14\n{body}");
        assert_eq!(osu_circles(text.as_bytes()), Err(RoseaError::EmptyBeatmap));
    }
}

#[test]
fn rejects_missing_malformed_and_non_finite_object_fields() {
    for (index, field, values) in [
        (0, "x", vec!["", "abc", "NaN", "inf", "-inf", "1e99"]),
        (1, "y", vec!["", "abc", "NaN", "inf", "-inf", "1e99"]),
        (2, "time", vec!["", "abc", "NaN", "inf", "-inf", "1e999"]),
        (3, "type", vec!["", "abc", "-1", "1.5", "256"]),
        (4, "hitSound", vec!["", "abc", "-1", "1.5", "256"]),
    ] {
        for value in values {
            let mut fields = ["256", "192", "1000", "1", "0"];
            fields[index] = value;
            let text = map(&format!("0,0,0,1,0\n{}", fields.join(",")));
            assert_eq!(
                osu_circles(text.as_bytes()),
                Err(RoseaError::InvalidHitObjectField {
                    field,
                    line: 4,
                    object_id: ObjectId(1)
                }),
                "{field}: {value}"
            );
        }
    }
    for (object, field) in [
        ("256", "y"),
        ("256,192", "time"),
        ("256,192,1000", "type"),
        ("256,192,1000,1", "hitSound"),
    ] {
        assert_eq!(
            osu_circles(map(object).as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field,
                line: 3,
                object_id: ObjectId(0)
            })
        );
    }
}

#[test]
fn rejects_other_and_ambiguous_object_types_instead_of_skipping_them() {
    for flags in [0, 2, 4, 8, 128, 3, 9, 129, 11, 255] {
        let text = map(&format!("0,0,0,1,0\n256,192,1000,{flags},0"));
        assert_eq!(
            osu_circles(text.as_bytes()),
            Err(RoseaError::UnsupportedHitObject {
                line: 4,
                object_id: ObjectId(1),
                flags
            })
        );
    }
}

#[test]
fn rejects_object_parameters_mislabeled_as_circle_samples() {
    for sample in [
        "1000",
        "B|300:200,1,100",
        "0:0:0",
        "0:0:abc:0:",
        "0:0:0:-1:",
    ] {
        let text = map(&format!("256,192,1000,1,0,{sample}"));
        assert_eq!(
            osu_circles(text.as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field: "hitSample",
                line: 3,
                object_id: ObjectId(0)
            })
        );
    }
}

#[test]
fn checks_header_and_standard_mode() {
    assert_eq!(osu_circles(b"\xff"), Err(RoseaError::InvalidEncoding));
    assert_eq!(osu_circles(b"[HitObjects]"), Err(RoseaError::InvalidHeader));
    assert_eq!(
        osu_circles(b"osu file format v13"),
        Err(RoseaError::UnsupportedVersion(13))
    );
    let text = "osu file format v14\n[General]\nMode: 1\n[HitObjects]\n256,192,1000,1,0";
    assert_eq!(
        osu_circles(text.as_bytes()),
        Err(RoseaError::UnsupportedMode(1))
    );
}

#[test]
fn enforces_object_and_line_limits() {
    let mut text = map(&"0,0,0,1,0\n".repeat(100_000));
    assert_eq!(osu_circles(text.as_bytes()).unwrap().len(), 100_000);
    text.push_str("0,0,0,1,0");
    assert_eq!(
        osu_circles(text.as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "hit_objects",
            limit: 100_000,
            observed: 100_001
        })
    );

    let prefix = "0,0,0,1,0,0:0:0:0:";
    let mut object = prefix.to_owned();
    object.push_str(&"a".repeat(1024 * 1024 - prefix.len()));
    assert!(osu_circles(map(&object).as_bytes()).is_ok());
    object.push('a');
    assert_eq!(
        osu_circles(map(&object).as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "osu_line_bytes",
            limit: 1_048_576,
            observed: 1_048_577
        })
    );
}
