//! Дуги окружности, вырожденные P-пути и их временная шкала.

use std::f64::consts::PI;

use rosea_core::error::RoseaError;
use rosea_core::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};
use rosea_core::parse::{osu_difficulty, osu_hit_objects, osu_timing_points};
use rosea_core::prepare::{
    PreparedPath, SliderEventKind, linear_path, linear_sliders, slider_path, sliders,
};

fn object(controls: &[(f32, f32)], length: f64) -> HitObject {
    HitObject {
        id: ObjectId(5),
        line: 7,
        position: Point {
            x: controls[0].0,
            y: controls[0].1,
        },
        start_time: 0.0,
        flags: 2,
        hit_sound: 0,
        kind: HitObjectKind::Slider {
            kind: CurveKind::PerfectCurve,
            controls: controls.iter().map(|&(x, y)| Point { x, y }).collect(),
            spans: 1,
            declared_length: length,
        },
    }
}

fn close_point(actual: Point, expected: Point) {
    assert!(
        (actual.x - expected.x).abs() <= 1e-4,
        "{actual:?} != {expected:?}"
    );
    assert!(
        (actual.y - expected.y).abs() <= 1e-4,
        "{actual:?} != {expected:?}"
    );
}

// Координаты считаются для заранее известной окружности, без восстановления её по трём точкам.
fn circle_vertices(
    center: Point,
    angles: (f64, f64),
    segments: usize,
    ends: [Point; 2],
) -> Vec<Point> {
    let mut vertices: Vec<_> = (0..=segments)
        .map(|index| {
            let angle = angles.0 + angles.1 * (index as f64 / segments as f64);
            Point {
                x: (f64::from(center.x) + 100.0 * angle.cos()) as f32,
                y: (f64::from(center.y) + 100.0 * angle.sin()) as f32,
            }
        })
        .collect();
    vertices[0] = ends[0];
    vertices[segments] = ends[1];
    vertices
}

fn polyline_length(vertices: &[Point]) -> f64 {
    vertices
        .windows(2)
        .map(|pair| {
            let dx = f64::from(pair[1].x) - f64::from(pair[0].x);
            let dy = f64::from(pair[1].y) - f64::from(pair[0].y);
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

fn compare_arc(controls: &[(f32, f32)], expected: &[Point]) -> PreparedPath {
    let length = polyline_length(expected);
    let path = slider_path(&object(controls, length)).unwrap();
    assert_eq!(path.vertices().len(), expected.len());
    for (&actual, &expected) in path.vertices().iter().zip(expected) {
        close_point(actual, expected);
    }
    assert!((path.length() - length).abs() <= 1e-3);
    assert!(
        path.cumulative_lengths()
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    assert_eq!(path.position_at(0.0).unwrap(), expected[0]);
    assert_eq!(path.position_at(1.0).unwrap(), *expected.last().unwrap());
    path
}

#[test]
fn semicircles_use_36_segments_in_both_directions_with_exact_ends() {
    for sign in [-1.0_f32, 1.0] {
        let expected = circle_vertices(
            Point { x: -100.0, y: 0.0 },
            (0.0, f64::from(sign) * PI),
            36,
            [Point { x: 0.0, y: 0.0 }, Point { x: -200.0, y: 0.0 }],
        );
        let path = compare_arc(
            &[(100.0, 0.0), (0.0, sign * 100.0), (-100.0, 0.0)],
            &expected,
        );
        assert_eq!(path.origin(), Point { x: 100.0, y: 0.0 });
        close_point(
            path.position_at(0.5).unwrap(),
            Point {
                x: -100.0,
                y: sign * 100.0,
            },
        );
        for pair in path.vertices().windows(2) {
            let x = (f64::from(pair[0].x) + f64::from(pair[1].x)) / 2.0 + 100.0;
            let y = (f64::from(pair[0].y) + f64::from(pair[1].y)) / 2.0;
            assert!(100.0 - (x * x + y * y).sqrt() <= 0.1 + 1e-4);
        }
    }
}

#[test]
fn middle_point_selects_the_major_arc_in_either_direction() {
    for sign in [-1.0_f32, 1.0] {
        let expected = circle_vertices(
            Point { x: -100.0, y: 0.0 },
            (0.0, -f64::from(sign) * 1.5 * PI),
            53,
            [
                Point { x: 0.0, y: 0.0 },
                Point {
                    x: -100.0,
                    y: sign * 100.0,
                },
            ],
        );
        let path = compare_arc(
            &[(100.0, 0.0), (-100.0, 0.0), (0.0, sign * 100.0)],
            &expected,
        );
        assert!(path.length() > 470.0);
        assert!(path.vertices()[1].y * sign < 0.0);
        assert!(path.vertices().iter().any(|point| point.x < -199.0));
    }
}

#[test]
fn arc_crosses_the_angle_wrap_and_keeps_translated_coordinates_relative() {
    let expected = circle_vertices(
        Point { x: 100.0, y: 0.0 },
        (PI, PI),
        36,
        [Point { x: 0.0, y: 0.0 }, Point { x: 200.0, y: 0.0 }],
    );
    let path = compare_arc(&[(100.0, 200.0), (200.0, 100.0), (300.0, 200.0)], &expected);
    assert_eq!(path.origin(), Point { x: 100.0, y: 200.0 });
    close_point(
        path.position_at(0.5).unwrap(),
        Point {
            x: 100.0,
            y: -100.0,
        },
    );
}

#[test]
fn collinear_points_keep_the_middle_and_backtracking_in_the_linear_fallback() {
    let path = slider_path(&object(&[(0.0, 0.0), (100.0, 0.0), (50.0, 0.0)], 150.0)).unwrap();
    assert_eq!(
        path.vertices(),
        &[
            Point { x: 0.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
            Point { x: 50.0, y: 0.0 }
        ]
    );
    assert_eq!(path.cumulative_lengths(), &[0.0, 100.0, 150.0]);
    assert_eq!(
        path.position_at_distance(125.0).unwrap(),
        Point { x: 75.0, y: 0.0 }
    );
    for (controls, length, end) in [
        ([(0.0, 0.0), (100.0, 0.0), (100.0, 0.0)], 150.0, 100.0),
        ([(0.0, 0.0), (0.0, 0.0), (100.0, 0.0)], 150.0, 150.0),
        ([(0.0, 0.0), (100.0, 0.0), (0.0, 0.0)], 200.0, 0.0),
        ([(0.0, 0.0); 3], 100.0, 0.0),
    ] {
        let path = slider_path(&object(&controls, length)).unwrap();
        assert_eq!(path.position_at(1.0).unwrap(), Point { x: end, y: 0.0 });
        if controls[1] == controls[2] {
            assert_eq!(path.length(), f64::from(end));
        }
    }
    // До проверки коллинеарности применяется та же нормализация, что и у других путей.
    let path = slider_path(&object(
        &[(100.9, 20.9), (150.2, 20.8), (125.9, 20.1)],
        75.0,
    ))
    .unwrap();
    assert_eq!(path.origin(), Point { x: 100.0, y: 20.0 });
    assert_eq!(
        path.vertices(),
        &[
            Point { x: 0.0, y: 0.0 },
            Point { x: 50.0, y: 0.0 },
            Point { x: 25.0, y: 0.0 }
        ]
    );
}

#[test]
fn other_control_counts_match_bezier_including_segment_splitting() {
    for controls in [
        vec![(0.0, 0.0)],
        vec![(0.0, 0.0), (100.0, 0.0)],
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 0.0), (100.0, 100.0)],
        vec![(0.0, 0.0), (0.0, 100.0), (100.0, 100.0), (100.0, 0.0)],
    ] {
        let perfect = object(&controls, 200.0);
        let mut bezier = object(&controls, 200.0);
        if let HitObjectKind::Slider { kind, .. } = &mut bezier.kind {
            *kind = CurveKind::Bezier;
        }
        let p_path = slider_path(&perfect).unwrap();
        let b_path = slider_path(&bezier).unwrap();
        assert_eq!(p_path.vertices(), b_path.vertices());
        assert_eq!(p_path.cumulative_lengths(), b_path.cumulative_lengths());
        assert_eq!(p_path.length(), b_path.length());
    }
}

#[test]
fn small_radius_uses_the_chord_error_tolerance_to_choose_segment_count() {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let expected = [
        Point { x: 0.0, y: 0.0 },
        Point {
            x: (-1.0 + half) as f32,
            y: half as f32,
        },
        Point { x: -1.0, y: 1.0 },
        Point {
            x: (-1.0 - half) as f32,
            y: half as f32,
        },
        Point { x: -2.0, y: 0.0 },
    ];
    compare_arc(&[(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)], &expected);
}

#[test]
fn declared_length_trims_extends_or_collapses_the_approximated_arc() {
    let controls = [(100.0, 0.0), (0.0, 100.0), (-100.0, 0.0)];
    let reference = circle_vertices(
        Point { x: -100.0, y: 0.0 },
        (0.0, PI),
        36,
        [Point { x: 0.0, y: 0.0 }, Point { x: -200.0, y: 0.0 }],
    );
    let length = polyline_length(&reference);
    let trimmed = slider_path(&object(&controls, length / 2.0)).unwrap();
    close_point(
        trimmed.position_at(1.0).unwrap(),
        Point {
            x: -100.0,
            y: 100.0,
        },
    );
    assert_eq!(trimmed.length(), length / 2.0);
    assert_eq!(*trimmed.cumulative_lengths().last().unwrap(), length / 2.0);
    let extended = slider_path(&object(&controls, length + 50.0)).unwrap();
    assert_eq!(extended.vertices().len(), 38);
    let last = reference[36];
    let previous = reference[35];
    let dx = f64::from(last.x) - f64::from(previous.x);
    let dy = f64::from(last.y) - f64::from(previous.y);
    let segment = (dx * dx + dy * dy).sqrt();
    close_point(
        extended.position_at(1.0).unwrap(),
        Point {
            x: (f64::from(last.x) + dx / segment * 50.0) as f32,
            y: (f64::from(last.y) + dy / segment * 50.0) as f32,
        },
    );
    assert_eq!(
        *extended.cumulative_lengths().last().unwrap(),
        length + 50.0
    );
    let zero = slider_path(&object(&controls, 0.0)).unwrap();
    assert_eq!(zero.length(), 0.0);
    assert_eq!(zero.vertices(), &[Point { x: 0.0, y: 0.0 }]);
}

#[test]
fn nearly_collinear_points_fail_at_the_limit_without_an_epsilon_fallback() {
    let large_arc = object(&[(0.0, 0.0), (1_000_000.0, 1.0), (999_999.0, 1.0)], 0.0);
    assert!(
        matches!(slider_path(&large_arc), Err(RoseaError::ResourceLimit {
        resource: "prepared_vertices", limit: 2_000_000, observed,
    }) if observed > 2_000_000 && observed < u64::MAX)
    );
    let unresolved_step = object(
        &[(0.0, 0.0), (514_229.0, 317_811.0), (832_040.0, 514_229.0)],
        0.0,
    );
    assert_eq!(
        slider_path(&unresolved_step).unwrap_err(),
        RoseaError::ResourceLimit {
            resource: "prepared_vertices",
            limit: 2_000_000,
            observed: u64::MAX,
        }
    );
}

#[test]
fn validates_inputs_and_keeps_the_linear_entry_points_strict() {
    let controls = [(100.0, 0.0), (0.0, 100.0), (-100.0, 0.0)];
    let source = object(&controls, 300.0);
    let map = b"osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\nSliderMultiplier:1\nSliderTickRate:1";
    assert!(matches!(
        linear_path(&source),
        Err(RoseaError::InvalidHitObjectField { field: "type", .. })
    ));
    assert!(matches!(
        linear_sliders(&[source], &osu_difficulty(map).unwrap(), &[]),
        Err(RoseaError::InvalidHitObjectField { field: "type", .. })
    ));
    for coordinate in [f32::NAN, f32::INFINITY, 1_000_000.25] {
        assert_eq!(
            slider_path(&object(
                &[(0.0, 0.0), (coordinate, 0.0), (0.0, 100.0)],
                100.0
            ))
            .unwrap_err(),
            RoseaError::InvalidHitObjectField {
                field: "x",
                line: 7,
                object_id: ObjectId(5),
            }
        );
    }
    for length in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            slider_path(&object(&controls, length)),
            Err(RoseaError::InvalidSliderLength { .. })
        ));
    }
    assert!(matches!(
        slider_path(&object(&controls, f64::MAX)),
        Err(RoseaError::NumericalError { .. })
    ));
}

#[test]
fn parsed_arc_reuses_its_path_for_ticks_repeats_and_ball_motion() {
    let reference = circle_vertices(
        Point { x: -100.0, y: 0.0 },
        (0.0, PI),
        36,
        [Point { x: 0.0, y: 0.0 }, Point { x: -200.0, y: 0.0 }],
    );
    let length = polyline_length(&reference);
    let map = format!(
        "osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,100,4,0,0,100,1,0\n[HitObjects]\n100,0,0,2,0,P|0:100|-100:0,2,{length}\n0,0,1000,2,0,L|100:0,1,100\n0,0,1000,2,0,B|50:100|100:0,1,100\n0,0,0,1,0"
    );
    let objects = osu_hit_objects(map.as_bytes()).unwrap();
    let prepared = sliders(
        &objects,
        &osu_difficulty(map.as_bytes()).unwrap(),
        &osu_timing_points(map.as_bytes()).unwrap(),
    )
    .unwrap();
    assert_eq!(prepared.len(), 3);
    assert_eq!(
        prepared
            .iter()
            .map(|slider| slider.timing().object_id)
            .collect::<Vec<_>>(),
        vec![ObjectId(0), ObjectId(1), ObjectId(2)]
    );
    let slider = &prepared[0];
    let timing = slider.timing();
    assert!((timing.duration - 2.0 * length).abs() <= 1e-9);
    assert!((timing.tail_check_time - (2.0 * length - 36.0)).abs() <= 1e-9);
    assert_eq!(timing.end_position, Point { x: 0.0, y: 0.0 });
    let ticks: Vec<_> = timing
        .events
        .iter()
        .filter(|event| event.kind == SliderEventKind::Tick)
        .collect();
    assert_eq!(ticks.len(), 6);
    for (event, expected) in ticks.iter().zip([
        100.0,
        200.0,
        300.0,
        2.0 * length - 300.0,
        2.0 * length - 200.0,
        2.0 * length - 100.0,
    ]) {
        assert!((event.time - expected).abs() <= 1e-9);
        close_point(
            event.position,
            slider.path().position_at(event.path_progress).unwrap(),
        );
    }
    let repeat = timing
        .events
        .iter()
        .find(|event| event.kind == SliderEventKind::Repeat)
        .unwrap();
    assert_eq!(repeat.time, length);
    assert_eq!(repeat.position, Point { x: -200.0, y: 0.0 });
    close_point(
        slider.position_at_time(length / 2.0).unwrap(),
        Point {
            x: -100.0,
            y: 100.0,
        },
    );
    close_point(
        slider.position_at_time(1.5 * length).unwrap(),
        Point {
            x: -100.0,
            y: 100.0,
        },
    );
    assert_eq!(
        slider.position_at_time(2.0 * length + 1.0).unwrap(),
        Point { x: 0.0, y: 0.0 }
    );
}
