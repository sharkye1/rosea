//! Высоты стека, граничные времена и смещение общей геометрии кругов и слайдеров.

use rosea_core::error::RoseaError;
use rosea_core::model::{HitObject, HitObjectKind, ObjectId, Point};
use rosea_core::parse::{
    Difficulty, osu_difficulty, osu_hit_objects, osu_stack_leniency, osu_timing_points,
};
use rosea_core::prepare::{StackedObject, sliders, stack_objects};

fn map(objects: &str, cs: f64, ar: f64, leniency: f32) -> String {
    format!(
        "osu file format v14\n[General]\nStackLeniency:{leniency}\n[Difficulty]\nCircleSize:{cs}\nOverallDifficulty:5\nHPDrainRate:5\nApproachRate:{ar}\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,1000,4,0,0,100,1,0\n[HitObjects]\n{objects}"
    )
}

fn run(objects: &str, cs: f64, ar: f64, leniency: f32) -> Vec<StackedObject> {
    let text = map(objects, cs, ar, leniency);
    let bytes = text.as_bytes();
    stack_objects(
        &osu_hit_objects(bytes).unwrap(),
        &osu_difficulty(bytes).unwrap(),
        &osu_timing_points(bytes).unwrap(),
        osu_stack_leniency(bytes).unwrap(),
    )
    .unwrap()
}

fn heights(objects: &[StackedObject]) -> Vec<i32> {
    objects.iter().map(StackedObject::height).collect()
}

fn difficulty() -> Difficulty {
    osu_difficulty(map("0,0,0,1,0", 5.0, 5.0, 0.7).as_bytes()).unwrap()
}

#[test]
fn three_circles_use_the_expected_height_and_cs_scale() {
    let objects = run(
        "100,100,0,1,0\n100,100,100,1,0\n100,100,200,1,0",
        5.0,
        5.0,
        0.7,
    );
    assert_eq!(heights(&objects), vec![2, 1, 0]);
    let step = -3.201_311_8_f32;
    for (object, height) in objects.iter().zip([2, 1, 0]) {
        let offset = height as f32 * step;
        assert_eq!(
            object.offset(),
            Point {
                x: offset,
                y: offset
            }
        );
        assert_eq!(
            object.position(),
            Point {
                x: 100.0 + offset,
                y: 100.0 + offset
            }
        );
        assert_eq!(object.end_position(), object.position());
        assert_eq!(object.position_at_time(50.0).unwrap(), object.position());
        assert!(object.slider().is_none());
    }
}

#[test]
fn time_threshold_includes_equality_and_tracks_the_current_chain_member() {
    for (gap, expected) in [(840, vec![1, 0]), (841, vec![0, 0])] {
        assert_eq!(
            heights(&run(
                &format!("100,100,0,1,0\n100,100,{gap},1,0"),
                5.0,
                5.0,
                0.7
            )),
            expected
        );
    }
    let chain = run(
        "100,100,0,1,0\n100,100,800,1,0\n100,100,1600,1,0",
        5.0,
        5.0,
        0.7,
    );
    assert_eq!(heights(&chain), vec![2, 1, 0]);
    for (ar, boundary, leniency) in [
        (0.0, 1260, 0.7),
        (10.0, 315, 0.7),
        (4.99, 1201, 1.0),
        (5.01, 1198, 1.0),
    ] {
        for (gap, expected) in [(boundary, vec![1, 0]), (boundary + 1, vec![0, 0])] {
            assert_eq!(
                heights(&run(
                    &format!("100,100,0,1,0\n100,100,{gap},1,0"),
                    5.0,
                    ar,
                    leniency
                )),
                expected,
                "AR={ar}, gap={gap}"
            );
        }
    }
}

#[test]
fn circle_time_truncates_towards_zero_but_slider_time_stays_fractional() {
    for start in [0.01, -0.99] {
        let circles = run(
            &format!("100,100,{start},1,0\n100,100,840.999,1,0"),
            5.0,
            5.0,
            0.7,
        );
        assert_eq!(heights(&circles), vec![1, 0]);
        let slider = run(
            &format!("100,100,{start},1,0\n100,100,840.999,2,0,L|200:100,1,100"),
            5.0,
            5.0,
            0.7,
        );
        assert_eq!(heights(&slider), vec![0, 0]);
    }
    // Для круга отбрасывается дробная часть конца предыдущего слайдера, а не его начала.
    let tail = run(
        "0,0,0.9,2,0,L|100:0,1,100\n100,0,1840.999,1,0",
        5.0,
        5.0,
        0.7,
    );
    assert_eq!(heights(&tail), vec![0, -1]);
}

#[test]
fn distance_three_is_excluded_after_coordinate_normalization() {
    for (x, y, expected) in [
        (3.0, 0.0, vec![0, 0]),
        (2.0, 2.0, vec![1, 0]),
        (3.0, 1.0, vec![0, 0]),
        (2.99, 0.0, vec![1, 0]),
    ] {
        assert_eq!(
            heights(&run(&format!("0,0,0,1,0\n{x},{y},100,1,0"), 5.0, 5.0, 0.7)),
            expected
        );
    }
    let clamped = run("600.9,600.9,0,1,0\n512,512,100,1,0", 5.0, 5.0, 0.7);
    assert_eq!(heights(&clamped), vec![1, 0]);
    assert_eq!(clamped[0].position().x, clamped[0].position().y);
    assert_eq!(clamped[1].position(), Point { x: 512.0, y: 512.0 });
}

#[test]
fn slider_tail_gives_later_circles_negative_heights() {
    let objects = run("0,0,0,2,0,L|100:0,1,100\n100,0,1100,1,0", 5.0, 5.0, 0.7);
    assert_eq!(heights(&objects), vec![0, -1]);
    assert_eq!(objects[0].end_position(), Point { x: 100.0, y: 0.0 });
    let offset = 3.201_311_8_f32;
    assert_eq!(
        objects[1].position(),
        Point {
            x: 100.0 + offset,
            y: offset
        }
    );
    let chain = run(
        "0,0,0,2,0,L|100:0,1,100\n100,0,1100,1,0\n300,300,1150,1,0\n100,0,1200,1,0\n100,0,1300,1,0",
        5.0,
        5.0,
        0.7,
    );
    assert_eq!(heights(&chain), vec![0, -1, 0, -2, -3]);
    // Уже назначенная высота предыдущего слайдера участвует в поправке хвостового стека.
    // Его конец сравнивается до смещения, иначе круг на x=102 перестал бы быть близким.
    let raised_slider = run(
        "0,0,0,2,0,L|100:0,1,100\n102,0,1100,1,0\n98,0,1200,2,0,L|198:0,1,100",
        5.0,
        0.0,
        0.7,
    );
    assert_eq!(heights(&raised_slider), vec![1, 0, 0]);
}

#[test]
fn end_position_uses_pass_parity_and_slider_stacks_compare_previous_ends() {
    for (spans, x, time, expected) in [
        (2, 0, 2100, vec![0, -1]),
        (2, 100, 2100, vec![0, 0]),
        (3, 100, 3100, vec![0, -1]),
    ] {
        let objects = run(
            &format!("0,0,0,2,0,L|100:0,{spans},100\n{x},0,{time},1,0"),
            5.0,
            5.0,
            0.7,
        );
        assert_eq!(heights(&objects), expected);
    }
    for (head, expected) in [(100, vec![1, 0]), (0, vec![0, 0])] {
        let objects = run(
            &format!("0,0,0,2,0,L|100:0,1,100\n{head},0,100,2,0,L|200:0,1,100"),
            5.0,
            5.0,
            0.7,
        );
        assert_eq!(heights(&objects), expected);
    }
}

#[test]
fn shifting_slider_origin_moves_the_whole_path_and_preserves_relative_geometry() {
    for curve in ["L|200:100", "B|150:200|200:100", "P|150:150|200:100"] {
        let text = map(
            &format!("100,100,0,2,0,{curve},1,100\n100,100,100,1,0"),
            5.0,
            5.0,
            0.7,
        );
        let source = osu_hit_objects(text.as_bytes()).unwrap();
        let difficulty = osu_difficulty(text.as_bytes()).unwrap();
        let points = osu_timing_points(text.as_bytes()).unwrap();
        let old = sliders(&source, &difficulty, &points).unwrap();
        let stacked = stack_objects(&source, &difficulty, &points, 0.7).unwrap();
        assert_eq!(heights(&stacked), vec![1, 0]);
        let object = &stacked[0];
        let new = object.slider().unwrap();
        assert_eq!(new.path().origin(), object.position());
        assert_eq!(new.path().vertices(), old[0].path().vertices());
        assert_eq!(
            new.path().cumulative_lengths(),
            old[0].path().cumulative_lengths()
        );
        assert_eq!(new.timing().events, old[0].timing().events);
        assert_eq!(new.timing().duration, old[0].timing().duration);
        for time in [0.0, 250.0, 500.0, 1000.0, 1100.0] {
            let relative = old[0].position_at_time(time).unwrap();
            assert_eq!(
                object.position_at_time(time).unwrap(),
                Point {
                    x: object.position().x + relative.x,
                    y: object.position().y + relative.y
                }
            );
        }
        assert_eq!(
            object.end_position(),
            object.position_at_time(new.timing().end_time).unwrap()
        );
        assert_eq!(source[0].position, Point { x: 100.0, y: 100.0 });
        // Повторная подготовка исходных объектов не накапливает прежнее смещение.
        assert_eq!(
            heights(&stack_objects(&source, &difficulty, &points, 0.7).unwrap()),
            vec![1, 0]
        );
    }
}

#[test]
fn sorts_unsorted_objects_by_time_then_id_and_treats_signed_zero_as_equal() {
    let text = map(
        "100,100,0,1,0\n100,100,-0,2,0,L|200:100,1,100\n100,100,0,1,0",
        5.0,
        5.0,
        0.7,
    );
    let mut objects = osu_hit_objects(text.as_bytes()).unwrap();
    objects[1].start_time = -0.0;
    objects.reverse();
    let result = stack_objects(
        &objects,
        &osu_difficulty(text.as_bytes()).unwrap(),
        &osu_timing_points(text.as_bytes()).unwrap(),
        0.7,
    )
    .unwrap();
    assert_eq!(
        result
            .iter()
            .map(StackedObject::object_id)
            .collect::<Vec<_>>(),
        vec![ObjectId(0), ObjectId(1), ObjectId(2)]
    );
    assert_eq!(heights(&result), vec![2, 1, 0]);
    assert_eq!(result[1].slider().unwrap().timing().object_id, ObjectId(1));
}

#[test]
fn zero_leniency_still_allows_equal_times_and_overlapping_slider_tails() {
    for (time, expected) in [(0, vec![1, 0]), (1, vec![0, 0])] {
        assert_eq!(
            heights(&run(
                &format!("100,100,0,1,0\n100,100,{time},1,0"),
                5.0,
                5.0,
                0.0
            )),
            expected
        );
    }
    assert_eq!(
        heights(&run("0,0,0,2,0,L|100:0,1,100\n0,0,500,1,0", 5.0, 5.0, 0.0)),
        vec![1, 0]
    );
}

#[test]
fn validates_manual_inputs_and_nonfinite_position_queries() {
    let source = osu_hit_objects(b"osu file format v14\n[HitObjects]\n0,0,0,1,0").unwrap();
    for leniency in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        assert!(matches!(
            stack_objects(&source, &difficulty(), &[], leniency),
            Err(RoseaError::PreprocessError { .. })
        ));
    }
    for field in ["cs", "ar"] {
        for value in [-1.0, 11.0, f32::NAN, f32::INFINITY] {
            let mut settings = difficulty();
            if field == "cs" {
                settings.cs = value;
            } else {
                settings.ar = value;
            }
            assert!(matches!(
                stack_objects(&source, &settings, &[], 0.7),
                Err(RoseaError::PreprocessError { .. })
            ));
        }
    }
    for time in [f64::NAN, f64::INFINITY, 86_400_000.25] {
        let mut objects = osu_hit_objects(b"osu file format v14\n[HitObjects]\n0,0,0,1,0").unwrap();
        objects[0].start_time = time;
        assert!(stack_objects(&objects, &difficulty(), &[], 0.7).is_err());
    }
    for value in [f32::NAN, f32::INFINITY, 1_000_000.25] {
        let mut objects = osu_hit_objects(b"osu file format v14\n[HitObjects]\n0,0,0,1,0").unwrap();
        objects[0].position.y = value;
        assert_eq!(
            stack_objects(&objects, &difficulty(), &[], 0.7).unwrap_err(),
            RoseaError::InvalidHitObjectField {
                field: "y",
                line: 3,
                object_id: ObjectId(0)
            }
        );
    }
    let result = stack_objects(&source, &difficulty(), &[], 0.7).unwrap();
    assert_eq!(heights(&result), vec![0]);
    for time in [f64::NAN, f64::INFINITY] {
        assert_eq!(
            result[0].position_at_time(time).unwrap_err(),
            RoseaError::NumericalError {
                line: 3,
                object_id: ObjectId(0)
            }
        );
    }
    assert!(
        stack_objects(&[], &difficulty(), &[], 0.7)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejects_nonfinite_distance_calculations_instead_of_silently_skipping_them() {
    let text = map(
        "0,0,0,2,0,L|100:0,1,100000000000000000000\n0,0,100,1,0",
        5.0,
        5.0,
        0.7,
    );
    let objects = osu_hit_objects(text.as_bytes()).unwrap();
    let mut difficulty = osu_difficulty(text.as_bytes()).unwrap();
    difficulty.slider_multiplier = Some(1e16);
    assert_eq!(
        stack_objects(
            &objects,
            &difficulty,
            &osu_timing_points(text.as_bytes()).unwrap(),
            0.7
        )
        .unwrap_err(),
        RoseaError::NumericalError {
            line: objects[0].line,
            object_id: ObjectId(0)
        }
    );
}

#[test]
fn bounds_objects_and_the_shared_distance_check_budget() {
    let objects: Vec<_> = (0..100_001)
        .map(|id| HitObject {
            id: ObjectId(id),
            line: id as usize + 3,
            position: Point { x: 0.0, y: 0.0 },
            start_time: 0.0,
            flags: 1,
            hit_sound: 0,
            kind: HitObjectKind::Circle,
        })
        .collect();
    assert_eq!(
        stack_objects(&objects, &difficulty(), &[], 0.7).unwrap_err(),
        RoseaError::ResourceLimit {
            resource: "hit_objects",
            limit: 100_000,
            observed: 100_001
        }
    );
    let objects: Vec<_> = (0..7000)
        .map(|id| HitObject {
            id: ObjectId(id),
            line: id as usize + 3,
            position: Point {
                x: (id % 128 * 4) as f32,
                y: (id / 128 * 4) as f32,
            },
            start_time: 0.0,
            flags: 1,
            hit_sound: 0,
            kind: HitObjectKind::Circle,
        })
        .collect();
    assert_eq!(
        stack_objects(&objects, &difficulty(), &[], 0.7).unwrap_err(),
        RoseaError::ResourceLimit {
            resource: "stacking_checks",
            limit: 20_000_000,
            observed: 20_000_001
        }
    );
}
