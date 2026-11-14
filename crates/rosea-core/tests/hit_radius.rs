use rosea_core::{model::Point, parse::osu, prepare::prepare_map, sim::hit::inside_radius};

#[test]
fn includes_the_boundary_in_world_coordinates() {
    let center = Point { x: 100.0, y: 100.0 };
    assert!(inside_radius(Point { x: 132.0, y: 100.0 }, center, 32.0));
    assert!(!inside_radius(
        Point {
            x: 132.001,
            y: 100.0
        },
        center,
        32.0
    ));
    assert!(inside_radius(center, center, 0.0));
    assert!(!inside_radius(center, center, -1.0));
    assert!(!inside_radius(
        Point {
            x: f32::NAN,
            y: 0.0
        },
        center,
        32.0
    ));
    assert!(!inside_radius(
        Point {
            x: f32::MAX,
            y: 0.0
        },
        center,
        32.0
    ));
}

#[test]
fn uses_the_stacked_object_center() {
    let input = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:5\nHPDrainRate:5\n[HitObjects]\n100,100,1000,1,0\n100,100,1100,1,0";
    let prepared = prepare_map(&osu(input).unwrap().value).unwrap();
    let center = prepared.objects()[0].position();
    let cursor = Point {
        x: center.x - 32.0,
        y: center.y,
    };
    assert!(inside_radius(cursor, center, 32.0));
    assert!(!inside_radius(cursor, Point { x: 100.0, y: 100.0 }, 32.0));
}
