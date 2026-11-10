//! Чтение спиннеров, их поля, общие ID с кругами и слайдерами и интеграция со stacking.

use std::path::Path;

use rosea_core::error::RoseaError;
use rosea_core::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};
use rosea_core::parse::{
    osu_circles, osu_difficulty, osu_hit_objects, osu_stack_leniency, osu_timing_points,
};
use rosea_core::prepare::stack_objects;

fn map(objects: &str) -> String {
    format!("osu file format v14\n[HitObjects]\n{objects}")
}

#[test]
fn reads_spinner_with_all_fields_and_hit_sample() {
    let text = "\u{feff}osu file format v14\r\n[HitObjects]\r\n\
        256,192,2000,12,2,4000,0:0:0:0:\r\n\r\n// comment\r\n\
        256,192,1000,8,0,3000,0:0:0:0:sound,with,commas.wav\r\n";
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Ok(vec![
            HitObject {
                id: ObjectId(1),
                line: 6,
                position: Point { x: 256.0, y: 192.0 },
                start_time: 1000.0,
                flags: 8,
                hit_sound: 0,
                kind: HitObjectKind::Spinner { end: 3000.0 },
            },
            HitObject {
                id: ObjectId(0),
                line: 3,
                position: Point { x: 256.0, y: 192.0 },
                start_time: 2000.0,
                flags: 12,
                hit_sound: 2,
                kind: HitObjectKind::Spinner { end: 4000.0 },
            },
        ])
    );
}

#[test]
fn reads_spinner_without_hit_sample() {
    for line in [
        "256,192,1000,8,0,2500",
        "256,192,1000,8,0,2500,",
        " 256 , 192 , 1000 , 8 , 0 , 2500 ",
    ] {
        let text = map(line);
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Ok(vec![HitObject {
                id: ObjectId(0),
                line: 3,
                position: Point { x: 256.0, y: 192.0 },
                start_time: 1000.0,
                flags: 8,
                hit_sound: 0,
                kind: HitObjectKind::Spinner { end: 2500.0 },
            }])
        );
    }
}

#[test]
fn reads_mixed_objects_with_shared_ids_and_source_order_at_equal_times() {
    let text = "\u{feff}osu file format v14\r\n[HitObjects]\r\n\
        256,192,3000,8,0,5000\r\n\
        100,200,1000,1,0\r\n\
        256,192,1000,8,4,2000\r\n\
        0,0,2000,2,0,L|100:0,1,100\r\n";
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Ok(vec![
            HitObject {
                id: ObjectId(1),
                line: 4,
                position: Point { x: 100.0, y: 200.0 },
                start_time: 1000.0,
                flags: 1,
                hit_sound: 0,
                kind: HitObjectKind::Circle,
            },
            HitObject {
                id: ObjectId(2),
                line: 5,
                position: Point { x: 256.0, y: 192.0 },
                start_time: 1000.0,
                flags: 8,
                hit_sound: 4,
                kind: HitObjectKind::Spinner { end: 2000.0 },
            },
            HitObject {
                id: ObjectId(3),
                line: 6,
                position: Point { x: 0.0, y: 0.0 },
                start_time: 2000.0,
                flags: 2,
                hit_sound: 0,
                kind: HitObjectKind::Slider {
                    kind: CurveKind::Linear,
                    controls: vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }],
                    spans: 1,
                    declared_length: 100.0,
                },
            },
            HitObject {
                id: ObjectId(0),
                line: 3,
                position: Point { x: 256.0, y: 192.0 },
                start_time: 3000.0,
                flags: 8,
                hit_sound: 0,
                kind: HitObjectKind::Spinner { end: 5000.0 },
            },
        ])
    );
}

#[test]
fn preserves_fractional_positions_and_negative_time_and_canonicalizes_zero() {
    let text = map("10.5,-20.25,-500.5,8,0,-0\n0,0,-0,8,0,100.0");
    let objects = osu_hit_objects(text.as_bytes()).unwrap();
    assert_eq!(objects[0].position, Point { x: 10.5, y: -20.25 });
    assert_eq!(objects[0].start_time, -500.5);
    let HitObjectKind::Spinner { end } = objects[0].kind else {
        panic!("ожидался спиннер");
    };
    assert_eq!(end.to_bits(), 0.0_f64.to_bits());

    assert_eq!(objects[1].start_time.to_bits(), 0.0_f64.to_bits());
    let HitObjectKind::Spinner { end } = objects[1].kind else {
        panic!("ожидался спиннер");
    };
    assert_eq!(end, 100.0);
}

#[test]
fn rejects_missing_or_empty_end_time() {
    for line in [
        "256,192,1000,8,0",
        "256,192,1000,8,0,",
        "256,192,1000,8,0, , ",
    ] {
        let text = map(line);
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field: "endTime",
                line: 3,
                object_id: ObjectId(0),
            })
        );
    }
}

#[test]
fn rejects_malformed_and_non_finite_end_time() {
    for value in ["abc", "NaN", "inf", "-inf", "1e999"] {
        let text = map(&format!("256,192,1000,8,0,{value}"));
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field: "endTime",
                line: 3,
                object_id: ObjectId(0),
            })
        );
    }
}

#[test]
fn rejects_end_time_before_start_time() {
    let text = map("256,192,1000,8,0,999.9");
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Err(RoseaError::InvalidHitObjectField {
            field: "endTime",
            line: 3,
            object_id: ObjectId(0),
        })
    );

    // Одинаковые времена (нулевая длительность) допустимы по спецификации.
    let text = map("256,192,1000,8,0,1000.0");
    assert_eq!(
        osu_hit_objects(text.as_bytes()),
        Ok(vec![HitObject {
            id: ObjectId(0),
            line: 3,
            position: Point { x: 256.0, y: 192.0 },
            start_time: 1000.0,
            flags: 8,
            hit_sound: 0,
            kind: HitObjectKind::Spinner { end: 1000.0 },
        }])
    );
}

#[test]
fn rejects_invalid_hit_sample_on_spinner() {
    for sample in ["0:0:abc:0:", "0:0:0", "0:0:0:-1:", "extra,0:0:0:0:"] {
        let text = map(&format!("256,192,1000,8,0,2000,{sample}"));
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field: "hitSample",
                line: 3,
                object_id: ObjectId(0),
            })
        );
    }
}

#[test]
fn rejects_ambiguous_base_types_involving_spinners() {
    for flags in [9, 10, 11, 136] {
        let text = map(&format!("256,192,1000,{flags},0,2000"));
        assert_eq!(
            osu_hit_objects(text.as_bytes()),
            Err(RoseaError::UnsupportedHitObject {
                line: 3,
                object_id: ObjectId(0),
                flags,
            })
        );
    }
}

#[test]
fn osu_circles_still_rejects_spinners() {
    let text = map("256,192,1000,8,0,2000");
    assert_eq!(
        osu_circles(text.as_bytes()),
        Err(RoseaError::UnsupportedHitObject {
            line: 3,
            object_id: ObjectId(0),
            flags: 8,
        })
    );
}

#[test]
fn reads_real_corpus_maps_with_spinners() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../tests/data/03_spinners");
    if !root.exists() {
        return;
    }

    let test_cases = [
        ("realexit.osu", 1910, 3),
        ("1049018_Swiq_.osu", 1832, 4),
        ("2505634_Ancenthe.osu", 202, 1),
        ("2505634_k_1tty.osu", 202, 1),
        ("2513983_Dominik393.osu", 250, 2),
    ];

    for (filename, expected_total, expected_spinners) in test_cases {
        let path = root.join(filename);
        if !path.exists() {
            continue;
        }
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("Не удалось прочитать {filename}: {e}"));
        let objects =
            osu_hit_objects(&bytes).unwrap_or_else(|e| panic!("Ошибка парсинга {filename}: {e}"));

        assert_eq!(
            objects.len(),
            expected_total,
            "Неверное общее число объектов в {filename}"
        );
        let spinner_count = objects
            .iter()
            .filter(|o| matches!(o.kind, HitObjectKind::Spinner { .. }))
            .count();
        assert_eq!(
            spinner_count, expected_spinners,
            "Неверное число спиннеров в {filename}"
        );
    }
}

#[test]
fn stacking_preserves_spinner_coordinates_and_skips_in_chain() {
    let text = "\
osu file format v14
[General]
StackLeniency: 0.7
[Difficulty]
CircleSize: 5
OverallDifficulty: 5
ApproachRate: 5
HPDrainRate: 5
[HitObjects]
100,100,0,1,0
256,192,50,8,0,150
100,100,100,1,0
100,100,200,1,0
";
    let bytes = text.as_bytes();
    let objects = osu_hit_objects(bytes).unwrap();
    let difficulty = osu_difficulty(bytes).unwrap();
    let timing_points = osu_timing_points(bytes).unwrap();
    let leniency = osu_stack_leniency(bytes).unwrap();

    let stacked = stack_objects(&objects, &difficulty, &timing_points, leniency).unwrap();
    assert_eq!(stacked.len(), 4);

    // Спиннер (второй объект по времени 50) не должен иметь смещения
    let spinner = &stacked[1];
    assert_eq!(spinner.height(), 0);
    assert_eq!(spinner.offset(), Point { x: 0.0, y: 0.0 });
    assert_eq!(spinner.position(), Point { x: 256.0, y: 192.0 });

    // Круги должны образовывать стек между собой, пропуская спиннер: h = [2, 1, 0]
    assert_eq!(stacked[0].height(), 2);
    assert_eq!(stacked[2].height(), 1);
    assert_eq!(stacked[3].height(), 0);
}
