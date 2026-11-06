//! Нормативные L-примеры из SPEC §14.2 и границы численной подготовки пути.

use rosea_core::error::RoseaError;
use rosea_core::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};
use rosea_core::parse::osu_hit_objects;
use rosea_core::prepare::{PreparedPath, linear_path};

fn slider(points: &[(f32, f32)], length: f64) -> HitObject {
    let controls: Vec<_> = points.iter().map(|&(x, y)| Point { x, y }).collect();
    HitObject {
        id: ObjectId(7),
        line: 3,
        position: controls[0],
        start_time: 1000.0,
        flags: 2,
        hit_sound: 0,
        kind: HitObjectKind::Slider {
            kind: CurveKind::Linear,
            controls,
            spans: 1,
            declared_length: length,
        },
    }
}

fn path(points: &[(f32, f32)], length: f64) -> PreparedPath {
    linear_path(&slider(points, length)).unwrap()
}

#[test]
fn tc_003_length_trims_extends_and_collapses_to_zero() {
    for (length, end) in [(60.0, 60.0), (150.0, 150.0), (0.0, 0.0)] {
        let path = path(&[(0.0, 0.0), (100.0, 0.0)], length);
        assert_eq!(path.length(), length);
        assert_eq!(path.position_at(0.0).unwrap(), Point { x: 0.0, y: 0.0 });
        assert_eq!(
            path.position_at(1.0).unwrap(),
            Point {
                x: end as f32,
                y: 0.0
            }
        );
        assert_eq!(path.position_at(-2.0).unwrap(), Point { x: 0.0, y: 0.0 });
        assert_eq!(
            path.position_at(2.0).unwrap(),
            Point {
                x: end as f32,
                y: 0.0
            }
        );
    }
    let extended = path(&[(0.0, 0.0), (100.0, 0.0)], 150.0);
    assert_eq!(extended.cumulative_lengths(), &[0.0, 100.0, 150.0]);
}

#[test]
fn tc_003_duplicate_end_blocks_extension_but_allows_trimming() {
    let points = [(0.0, 0.0), (100.0, 0.0), (100.0, 0.0)];
    let extended = path(&points, 150.0);
    assert_eq!(extended.length(), 100.0);
    assert_eq!(
        extended.vertices(),
        &[Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]
    );
    assert_eq!(extended.cumulative_lengths(), &[0.0, 100.0]);
    let trimmed = path(&points, 60.0);
    assert_eq!(trimmed.length(), 60.0);
    assert_eq!(trimmed.position_at(1.0).unwrap(), Point { x: 60.0, y: 0.0 });
}

#[test]
fn tc_003_constant_has_zero_actual_length() {
    let path = path(&[(0.0, 0.0); 3], 100.0);
    assert_eq!(path.length(), 0.0);
    assert_eq!(path.vertices(), &[Point { x: 0.0, y: 0.0 }]);
    assert_eq!(path.cumulative_lengths(), &[0.0]);
    for progress in [-10.0, 0.0, 0.25, 1.0, 10.0] {
        assert_eq!(
            path.position_at(progress).unwrap(),
            Point { x: 0.0, y: 0.0 }
        );
    }
}

#[test]
fn samples_by_distance_along_unequal_segments() {
    let path = path(&[(0.0, 0.0), (30.0, 0.0), (30.0, 40.0)], 70.0);
    assert_eq!(path.cumulative_lengths(), &[0.0, 30.0, 70.0]);
    assert_eq!(path.position_at(0.5).unwrap(), Point { x: 30.0, y: 5.0 });
    for (distance, expected) in [
        (-1.0, Point { x: 0.0, y: 0.0 }),
        (30.0, Point { x: 30.0, y: 0.0 }),
        (55.0, Point { x: 30.0, y: 25.0 }),
        (100.0, Point { x: 30.0, y: 40.0 }),
    ] {
        assert_eq!(path.position_at_distance(distance).unwrap(), expected);
    }
}

#[test]
fn internal_duplicates_do_not_block_extension_of_the_last_segment() {
    let path = path(
        &[
            (0.0, 0.0),
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
        ],
        300.0,
    );
    assert_eq!(path.cumulative_lengths(), &[0.0, 100.0, 200.0, 300.0]);
    assert_eq!(path.position_at(1.0).unwrap(), Point { x: 100.0, y: 200.0 });
}

#[test]
fn trims_at_a_corner_or_inside_a_later_segment() {
    for (length, expected) in [
        (10.0, Point { x: 10.0, y: 0.0 }),
        (15.0, Point { x: 10.0, y: 5.0 }),
    ] {
        let path = path(
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (20.0, 10.0)],
            length,
        );
        assert_eq!(path.length(), length);
        assert_eq!(path.position_at(1.0).unwrap(), expected);
        assert_eq!(
            path.cumulative_lengths()[path.cumulative_lengths().len() - 1],
            length
        );
    }
}

#[test]
fn normalizes_origin_and_controls_before_making_them_relative() {
    for (start, control, origin, relative) in [
        (
            (600.75, 600.25),
            (550.99, -20.99),
            Point { x: 512.0, y: 512.0 },
            Point { x: 38.0, y: -532.0 },
        ),
        (
            (-30.7, -20.3),
            (-10.9, -15.9),
            Point { x: 0.0, y: 0.0 },
            Point { x: -10.0, y: -15.0 },
        ),
        (
            (100.9, 20.9),
            (150.9, 30.9),
            Point { x: 100.0, y: 20.0 },
            Point { x: 50.0, y: 10.0 },
        ),
    ] {
        let path = path(&[start, control], 10_000.0);
        assert_eq!(path.origin(), origin);
        assert_eq!(path.vertices()[0], Point { x: 0.0, y: 0.0 });
        assert_eq!(path.vertices()[1], relative);
    }
    // Флаг повторного конца определяется после trunc, когда дробные точки совпали.
    let path = path(&[(0.0, 0.0), (10.1, 0.0), (10.9, 0.0)], 20.0);
    assert_eq!(path.length(), 10.0);
}

#[test]
fn uses_f64_lengths_and_keeps_the_declared_prefix_after_f32_rounding() {
    let natural = path(&[(0.0, 0.0), (1.0, 1.0)], std::f64::consts::SQRT_2);
    assert_eq!(natural.length(), std::f64::consts::SQRT_2);
    let trimmed = path(&[(0.0, 0.0), (1.0, 1.0)], 1.0);
    let expected = Point {
        x: std::f32::consts::FRAC_1_SQRT_2,
        y: std::f32::consts::FRAC_1_SQRT_2,
    };
    assert_eq!(trimmed.position_at(1.0).unwrap(), expected);
    assert_eq!(trimmed.length(), 1.0);
    assert_eq!(trimmed.cumulative_lengths(), &[0.0, 1.0]);
    // Округлённый конец даёт другую геометрическую длину; префикс должен остаться ровно 1.
    let rounded_distance = (2.0 * f64::from(expected.x) * f64::from(expected.x)).sqrt();
    assert_ne!(rounded_distance, 1.0);
}

#[test]
fn rejects_invalid_source_data_and_coordinate_limits_before_normalization() {
    for length in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let object = slider(&[(0.0, 0.0), (1.0, 0.0)], length);
        assert_eq!(
            linear_path(&object).unwrap_err(),
            RoseaError::InvalidSliderLength {
                line: 3,
                object_id: ObjectId(7)
            }
        );
    }
    for value in [
        1_000_000.25,
        -1_000_000.25,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ] {
        let object = slider(&[(0.0, 0.0), (value, 0.0)], 1.0);
        assert_eq!(
            linear_path(&object).unwrap_err(),
            RoseaError::InvalidHitObjectField {
                field: "x",
                line: 3,
                object_id: ObjectId(7)
            }
        );
    }
    let at_limit = slider(
        &[(1_000_000.0, -1_000_000.0), (-1_000_000.0, 1_000_000.0)],
        1.0,
    );
    assert!(linear_path(&at_limit).is_ok());
    let too_many = slider(&vec![(0.0, 0.0); 4097], 0.0);
    assert_eq!(
        linear_path(&too_many).unwrap_err(),
        RoseaError::ResourceLimit {
            resource: "slider_control_points",
            limit: 4096,
            observed: 4097
        }
    );
    assert!(linear_path(&slider(&vec![(0.0, 0.0); 4096], 0.0)).is_ok());
    let mut object = slider(&[(0.0, 0.0)], 1.0);
    object.kind = HitObjectKind::Circle;
    assert_eq!(
        linear_path(&object).unwrap_err(),
        RoseaError::InvalidHitObjectField {
            field: "type",
            line: 3,
            object_id: ObjectId(7)
        }
    );
    object.kind = HitObjectKind::Slider {
        kind: CurveKind::Linear,
        controls: vec![],
        spans: 1,
        declared_length: 1.0,
    };
    assert_eq!(
        linear_path(&object).unwrap_err(),
        RoseaError::InvalidHitObjectField {
            field: "curvePoints",
            line: 3,
            object_id: ObjectId(7)
        }
    );
}

#[test]
fn reports_non_finite_computed_vertices_and_queries() {
    let object = slider(&[(0.0, 0.0), (1.0, 0.0)], f64::MAX);
    let expected = RoseaError::NumericalError {
        line: 3,
        object_id: ObjectId(7),
    };
    assert_eq!(linear_path(&object).unwrap_err(), expected);
    let path = path(&[(0.0, 0.0), (1.0, 0.0)], 1.0);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            path.position_at(invalid),
            Err(RoseaError::NumericalError {
                line: 3,
                object_id: ObjectId(7)
            })
        );
        assert_eq!(
            path.position_at_distance(invalid),
            Err(RoseaError::NumericalError {
                line: 3,
                object_id: ObjectId(7)
            })
        );
    }
}

#[test]
fn prepares_a_parsed_slider_without_applying_passes_to_its_geometry() {
    let objects = osu_hit_objects(
        b"osu file format v14\n[HitObjects]\n100.9,20.9,1000,2,0,L|150.9:20.9,3,80",
    )
    .unwrap();
    let path = linear_path(&objects[0]).unwrap();
    assert_eq!(path.origin(), Point { x: 100.0, y: 20.0 });
    assert_eq!(path.length(), 80.0);
    assert_eq!(path.position_at(1.0).unwrap(), Point { x: 80.0, y: 0.0 });
}
