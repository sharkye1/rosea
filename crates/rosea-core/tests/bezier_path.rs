//! Форма кривых Безье, длина пути и использование того же пути во временной шкале.

use rosea_core::error::RoseaError;
use rosea_core::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};
use rosea_core::parse::{osu_difficulty, osu_hit_objects, osu_timing_points};
use rosea_core::prepare::{SliderEventKind, linear_path, linear_sliders, slider_path, sliders};

fn object(controls: &[(f32, f32)], length: f64) -> HitObject {
    HitObject {
        id: ObjectId(7),
        line: 4,
        position: Point {
            x: controls[0].0,
            y: controls[0].1,
        },
        start_time: 0.0,
        flags: 2,
        hit_sound: 0,
        kind: HitObjectKind::Slider {
            kind: CurveKind::Bezier,
            controls: controls.iter().map(|&(x, y)| Point { x, y }).collect(),
            spans: 1,
            declared_length: length,
        },
    }
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= 1e-9, "{actual} != {expected}");
}

#[test]
fn duplicate_controls_split_segments_and_join_without_repeating_vertices() {
    let path = slider_path(&object(
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 0.0), (100.0, 100.0)],
        200.0,
    ))
    .unwrap();
    assert_eq!(
        path.vertices(),
        &[
            Point { x: 0.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
            Point { x: 100.0, y: 100.0 }
        ]
    );
    assert_eq!(path.cumulative_lengths(), &[0.0, 100.0, 200.0]);
    assert_eq!(path.position_at(0.5).unwrap(), Point { x: 100.0, y: 0.0 });
    assert_eq!(path.position_at(1.0).unwrap(), Point { x: 100.0, y: 100.0 });
}

#[test]
fn quadratic_has_the_expected_polyline_and_exact_endpoints() {
    // Для этой квадратичной кривой допуск достигается на пятом делении: 32 равных интервала.
    // Вершины считаем прямой формулой кривой, независимо от алгоритма разбиения.
    let expected: Vec<_> = (0..=32)
        .map(|index| {
            let t = f64::from(index) / 32.0;
            Point {
                x: (100.0 * t) as f32,
                y: (200.0 * t * (1.0 - t)) as f32,
            }
        })
        .collect();
    let length: f64 = expected
        .windows(2)
        .map(|pair| {
            let dx = f64::from(pair[1].x) - f64::from(pair[0].x);
            let dy = f64::from(pair[1].y) - f64::from(pair[0].y);
            (dx * dx + dy * dy).sqrt()
        })
        .sum();
    let path = slider_path(&object(&[(0.0, 0.0), (50.0, 100.0), (100.0, 0.0)], length)).unwrap();
    assert_eq!(path.vertices(), expected);
    close(path.length(), length);
    let middle = path.position_at(0.5).unwrap();
    assert!((middle.x - 50.0).abs() <= 1e-4);
    assert!((middle.y - 50.0).abs() <= 1e-4);
    assert_eq!(path.position_at(0.0).unwrap(), expected[0]);
    assert_eq!(path.position_at(1.0).unwrap(), expected[32]);
    assert!(
        path.cumulative_lengths()
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
}

#[test]
fn collinear_controls_still_use_parametrized_chord_flatness() {
    let path = slider_path(&object(&[(0.0, 0.0), (90.0, 0.0), (100.0, 0.0)], 100.0)).unwrap();
    assert_eq!(path.vertices().len(), 17);
    assert_eq!(path.vertices()[8], Point { x: 70.0, y: 0.0 });
    // Шарик движется по длине, поэтому половина пути не равна значению кривой при t=0.5.
    assert_eq!(path.position_at(0.5).unwrap(), Point { x: 50.0, y: 0.0 });
}

#[test]
fn cubic_preserves_its_shape_and_left_to_right_vertex_order() {
    let expected: Vec<_> = (0..=32)
        .map(|index| {
            let t = f64::from(index) / 32.0;
            Point {
                x: (100.0 * (3.0 * t * t - 2.0 * t * t * t)) as f32,
                y: (300.0 * t * (1.0 - t)) as f32,
            }
        })
        .collect();
    let length: f64 = expected
        .windows(2)
        .map(|pair| {
            let dx = f64::from(pair[1].x) - f64::from(pair[0].x);
            let dy = f64::from(pair[1].y) - f64::from(pair[0].y);
            (dx * dx + dy * dy).sqrt()
        })
        .sum();
    let path = slider_path(&object(
        &[(0.0, 0.0), (0.0, 100.0), (100.0, 100.0), (100.0, 0.0)],
        length,
    ))
    .unwrap();
    assert_eq!(path.vertices(), expected);
    let middle = path.position_at(0.5).unwrap();
    assert!((middle.x - 50.0).abs() <= 1e-4);
    assert!((middle.y - 75.0).abs() <= 1e-4);
}

#[test]
fn timed_curved_slider_uses_distance_rather_than_bezier_parameter() {
    let map = b"osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\nSliderMultiplier:1\nSliderTickRate:4\n[TimingPoints]\n0,100,4,0,0,100,1,0\n[HitObjects]\n0,0,0,2,0,B|90:0|100:0,2,100";
    let objects = osu_hit_objects(map).unwrap();
    let result = sliders(
        &objects,
        &osu_difficulty(map).unwrap(),
        &osu_timing_points(map).unwrap(),
    )
    .unwrap();
    let prepared = &result[0];
    assert_eq!(
        prepared.position_at_time(50.0).unwrap(),
        Point { x: 50.0, y: 0.0 }
    );
    assert_eq!(
        prepared.position_at_time(150.0).unwrap(),
        Point { x: 50.0, y: 0.0 }
    );
    assert_eq!(prepared.timing().duration, 200.0);
    let ticks: Vec<_> = prepared
        .timing()
        .events
        .iter()
        .filter(|event| event.kind == SliderEventKind::Tick)
        .map(|event| (event.time, event.position.x))
        .collect();
    assert_eq!(
        ticks,
        vec![
            (25.0, 25.0),
            (50.0, 50.0),
            (75.0, 75.0),
            (125.0, 75.0),
            (150.0, 50.0),
            (175.0, 25.0)
        ]
    );
}

#[test]
fn coincident_endpoints_preserve_the_outward_and_return_path() {
    let path = slider_path(&object(&[(0.0, 0.0), (100.0, 0.0), (0.0, 0.0)], 100.0)).unwrap();
    assert_eq!(path.length(), 100.0);
    assert_eq!(path.position_at(0.0).unwrap(), Point { x: 0.0, y: 0.0 });
    assert_eq!(path.position_at(0.5).unwrap(), Point { x: 50.0, y: 0.0 });
    assert_eq!(path.position_at(1.0).unwrap(), Point { x: 0.0, y: 0.0 });
}

#[test]
fn constants_repeated_endpoints_and_zero_declared_length_are_handled() {
    for controls in [&[(0.0, 0.0)][..], &[(0.0, 0.0); 4]] {
        let path = slider_path(&object(controls, 100.0)).unwrap();
        assert_eq!(path.length(), 0.0);
        assert_eq!(path.vertices(), &[Point { x: 0.0, y: 0.0 }]);
    }
    for controls in [
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 0.0)],
        vec![
            (0.0, 0.0),
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 0.0),
            (100.0, 0.0),
        ],
    ] {
        let path = slider_path(&object(&controls, 150.0)).unwrap();
        // Концевой разделитель не добавляет пустой сегмент или повторную вершину.
        assert_eq!(
            path.vertices(),
            &[
                Point { x: 0.0, y: 0.0 },
                Point { x: 100.0, y: 0.0 },
                Point { x: 150.0, y: 0.0 }
            ]
        );
        assert_eq!(path.length(), 150.0);
    }
    let zero = slider_path(&object(&[(0.0, 0.0), (50.0, 100.0), (100.0, 0.0)], 0.0)).unwrap();
    assert_eq!(zero.length(), 0.0);
    assert_eq!(zero.vertices(), &[Point { x: 0.0, y: 0.0 }]);
}

#[test]
fn normalizes_before_splitting_and_applies_declared_length_to_the_whole_path() {
    let path = slider_path(&object(
        &[(600.75, -2.5), (612.1, 0.9), (612.9, 0.1), (612.1, 100.9)],
        150.0,
    ))
    .unwrap();
    assert_eq!(path.origin(), Point { x: 512.0, y: 0.0 });
    assert_eq!(
        path.vertices(),
        &[
            Point { x: 0.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
            Point { x: 100.0, y: 50.0 }
        ]
    );
    assert_eq!(path.cumulative_lengths(), &[0.0, 100.0, 150.0]);
    assert_eq!(path.position_at(1.0).unwrap(), Point { x: 100.0, y: 50.0 });
    let extended = slider_path(&object(&[(0.0, 0.0), (50.0, 100.0), (100.0, 0.0)], 200.0)).unwrap();
    assert_eq!(extended.length(), 200.0);
    assert_eq!(*extended.cumulative_lengths().last().unwrap(), 200.0);
    assert!(extended.vertices().last().unwrap().x > 100.0);
    assert!(extended.vertices().last().unwrap().y < 0.0);
}

#[test]
fn rejects_invalid_geometry_and_preserves_linear_only_entry_points() {
    let source = object(&[(0.0, 0.0), (50.0, 100.0), (100.0, 0.0)], 100.0);
    assert_eq!(
        linear_path(&source).unwrap_err(),
        RoseaError::InvalidHitObjectField {
            field: "type",
            line: 4,
            object_id: ObjectId(7)
        }
    );
    let difficulty = osu_difficulty(
        b"osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6",
    )
    .unwrap();
    assert!(matches!(
        linear_sliders(&[source], &difficulty, &[]),
        Err(RoseaError::InvalidHitObjectField { field: "type", .. })
    ));
    for value in [f32::NAN, f32::INFINITY, 1_000_000.25] {
        assert_eq!(
            slider_path(&object(&[(0.0, 0.0), (value, 0.0)], 100.0)).unwrap_err(),
            RoseaError::InvalidHitObjectField {
                field: "x",
                line: 4,
                object_id: ObjectId(7)
            }
        );
    }
    for length in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            slider_path(&object(&[(0.0, 0.0), (100.0, 0.0)], length)),
            Err(RoseaError::InvalidSliderLength { .. })
        ));
    }
    assert!(matches!(
        slider_path(&object(&[(0.0, 0.0), (100.0, 0.0)], f64::MAX)),
        Err(RoseaError::NumericalError { .. })
    ));
    assert!(matches!(
        slider_path(&object(&vec![(0.0, 0.0); 4097], 0.0)),
        Err(RoseaError::ResourceLimit {
            resource: "slider_control_points",
            ..
        })
    ));
}

#[test]
fn parsed_mixed_map_uses_bezier_geometry_for_ticks_repeats_and_ball_motion() {
    let map = b"osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,100,4,0,0,100,1,0\n[HitObjects]\n0,0,300,2,0,L|100:0,1,100\n0,0,0,2,0,B|100:0|100:0|100:100,2,200\n0,0,0,1,0";
    let objects = osu_hit_objects(map).unwrap();
    let result = sliders(
        &objects,
        &osu_difficulty(map).unwrap(),
        &osu_timing_points(map).unwrap(),
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].timing().object_id, ObjectId(1));
    let prepared = &result[0];
    assert_eq!(prepared.timing().duration, 400.0);
    assert_eq!(prepared.timing().end_position, Point { x: 0.0, y: 0.0 });
    let events = &prepared.timing().events;
    assert_eq!(
        events
            .iter()
            .map(|event| (event.kind, event.time))
            .collect::<Vec<_>>(),
        vec![
            (SliderEventKind::Tick, 100.0),
            (SliderEventKind::Repeat, 200.0),
            (SliderEventKind::Tick, 300.0),
            (SliderEventKind::TailCheck, 364.0),
        ]
    );
    assert_eq!(events[0].position, Point { x: 100.0, y: 0.0 });
    assert_eq!(events[1].position, Point { x: 100.0, y: 100.0 });
    assert_eq!(events[2].position, Point { x: 100.0, y: 0.0 });
    assert_eq!(
        prepared.position_at_time(150.0).unwrap(),
        Point { x: 100.0, y: 50.0 }
    );
    assert_eq!(
        prepared.position_at_time(250.0).unwrap(),
        Point { x: 100.0, y: 50.0 }
    );
    assert_eq!(
        prepared.position_at_time(400.0).unwrap(),
        Point { x: 0.0, y: 0.0 }
    );
    assert_eq!(result[1].timing().object_id, ObjectId(0));
}

#[test]
fn interpolation_budget_is_shared_across_sliders() {
    // Каждый сегмент даёт 31 разбиение по три средних, всего 126852 операции на путь.
    // Нулевая объявленная длина убирает итоговые вершины, но не затраты построения кривой.
    let mut controls = vec![(0.0, 0.0)];
    for _ in 0..682 {
        controls.extend([
            (50.0, 100.0),
            (100.0, 0.0),
            (100.0, 0.0),
            (50.0, 100.0),
            (0.0, 0.0),
            (0.0, 0.0),
        ]);
    }
    assert!(slider_path(&object(&controls, 0.0)).is_ok());
    let objects: Vec<_> = (0..158)
        .map(|id| {
            let mut source = object(&controls, 0.0);
            source.id = ObjectId(id);
            source
        })
        .collect();
    let map = b"osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,100,4,0,0,100,1,0";
    assert!(matches!(
        sliders(
            &objects,
            &osu_difficulty(map).unwrap(),
            &osu_timing_points(map).unwrap()
        ),
        Err(RoseaError::ResourceLimit {
            resource: "bezier_interpolations",
            limit: 20_000_000,
            ..
        })
    ));
}
