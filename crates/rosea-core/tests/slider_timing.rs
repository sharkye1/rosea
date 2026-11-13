//! Времена слайдеров: изменения темпа, скорость, обратные проходы и вложенные части.

use rosea_core::error::RoseaError;
use rosea_core::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point, TimingPoint};
use rosea_core::parse::{Difficulty, osu_difficulty, osu_hit_objects, osu_timing_points};
use rosea_core::prepare::{PreparedSlider, SliderEventKind, linear_sliders};

fn difficulty(multiplier: f64, rate: f64) -> Difficulty {
    Difficulty {
        cs: 4.0,
        od: 5.0,
        hp: 6.0,
        ar: 5.0,
        slider_multiplier: Some(multiplier),
        slider_tick_rate: Some(rate),
    }
}

fn slider(start: f64, length: f64, spans: u32, id: u32) -> HitObject {
    HitObject {
        id: ObjectId(id),
        line: 10 + id as usize,
        position: Point { x: 0.0, y: 0.0 },
        start_time: start,
        flags: 2,
        hit_sound: 0,
        kind: HitObjectKind::Slider {
            kind: CurveKind::Linear,
            controls: vec![Point { x: 0.0, y: 0.0 }, Point { x: 1000.0, y: 0.0 }],
            spans,
            declared_length: length,
        },
    }
}

fn point(time: f64, beat: f64, red: bool, line: usize) -> TimingPoint {
    TimingPoint {
        line,
        time,
        beat_length: beat,
        meter: if red { 4 } else { 0 },
        sample_set: 0,
        sample_index: 0,
        volume: 100,
        uninherited: red,
        effects: 0,
    }
}

fn prepared(
    length: f64,
    spans: u32,
    multiplier: f64,
    rate: f64,
    points: &[TimingPoint],
) -> PreparedSlider {
    linear_sliders(
        &[slider(0.0, length, spans, 0)],
        &difficulty(multiplier, rate),
        points,
    )
    .unwrap()
    .remove(0)
}

fn close(actual: f64, expected: f64) {
    // Для дробных времён допускается абсолютная ошибка до одной миллиардной миллисекунды.
    assert!((actual - expected).abs() <= 1e-9, "{actual} != {expected}");
}

#[test]
fn computes_base_duration_tick_and_tail_without_rounding_milliseconds() {
    let slider = prepared(280.0, 1, 1.4, 1.0, &[point(0.0, 500.0, true, 1)]);
    let timing = slider.timing();
    assert_eq!(timing.beat_length, 500.0);
    assert_eq!(timing.speed_multiplier, 1.0);
    assert_eq!(timing.velocity, 0.28);
    assert_eq!(timing.tick_distance, 140.0);
    close(timing.span_duration, 1000.0);
    close(timing.duration, 1000.0);
    close(timing.end_time, 1000.0);
    // Сохраняется результат деления, без подгонки к ближайшей целой миллисекунде.
    assert_eq!(timing.end_time, 999.9999999999999);
    assert_eq!(timing.events.len(), 2);
    assert_eq!(timing.events[0].kind, SliderEventKind::Tick);
    close(timing.events[0].time, 500.0);
    assert_eq!(timing.events[0].path_progress, 0.5);
    assert_eq!(timing.events[0].position, Point { x: 140.0, y: 0.0 });
    assert_eq!(timing.events[1].kind, SliderEventKind::TailCheck);
    close(timing.tail_check_time, 964.0);
    assert_eq!(timing.end_position, Point { x: 280.0, y: 0.0 });
}

#[test]
fn inherited_speed_changes_duration_and_tick_distance() {
    let slider = prepared(
        280.0,
        1,
        1.4,
        1.0,
        &[point(0.0, 500.0, true, 1), point(0.0, -50.0, false, 2)],
    );
    let timing = slider.timing();
    assert_eq!(timing.speed_multiplier, 2.0);
    assert_eq!(timing.velocity, 0.56);
    assert_eq!(timing.tick_distance, 280.0);
    close(timing.span_duration, 500.0);
    assert_eq!(timing.events.len(), 1);
    assert_eq!(timing.events[0].kind, SliderEventKind::TailCheck);
}

#[test]
fn reverses_tick_times_and_ball_motion_on_alternate_passes() {
    let slider = prepared(350.0, 2, 1.4, 1.0, &[point(0.0, 500.0, true, 1)]);
    let timing = slider.timing();
    close(timing.span_duration, 1250.0);
    close(timing.duration, 2500.0);
    let parts: Vec<_> = timing
        .events
        .iter()
        .filter(|event| event.kind != SliderEventKind::TailCheck)
        .collect();
    let expected = [
        (500.0, SliderEventKind::Tick, 140.0),
        (1000.0, SliderEventKind::Tick, 280.0),
        (1250.0, SliderEventKind::Repeat, 350.0),
        (1500.0, SliderEventKind::Tick, 280.0),
        (2000.0, SliderEventKind::Tick, 140.0),
    ];
    assert_eq!(parts.len(), expected.len());
    for (event, (time, kind, x)) in parts.iter().zip(expected) {
        close(event.time, time);
        assert_eq!(event.kind, kind);
        assert_eq!(event.position, Point { x, y: 0.0 });
    }
    for (time, x) in [
        (-1.0, 0.0),
        (500.0, 140.0),
        (1250.0, 350.0),
        (1500.0, 280.0),
        (2500.0, 0.0),
        (5000.0, 0.0),
    ] {
        assert_eq!(slider.position_at_time(time).unwrap(), Point { x, y: 0.0 });
    }
    assert_eq!(timing.end_position, Point { x: 0.0, y: 0.0 });
    assert!(
        timing
            .events
            .windows(2)
            .all(|pair| pair[0].time <= pair[1].time)
    );
    let odd = prepared(350.0, 3, 1.4, 1.0, &[point(0.0, 500.0, true, 1)]);
    assert_eq!(odd.timing().end_position, Point { x: 350.0, y: 0.0 });
    assert_eq!(
        odd.timing()
            .events
            .iter()
            .filter(|event| event.kind == SliderEventKind::Repeat)
            .count(),
        2
    );
}

#[test]
fn equal_time_groups_use_first_red_and_last_green_by_source_line() {
    let values = [(-50.0, false), (500.0, true), (400.0, true), (-25.0, false)];
    for order in [[0, 1, 2, 3], [1, 0, 3, 2], [1, 2, 0, 3]] {
        let mut points: Vec<_> = order
            .iter()
            .enumerate()
            .map(|(line, &index)| point(0.0, values[index].0, values[index].1, line + 1))
            .collect();
        points.reverse();
        let slider = prepared(280.0, 1, 1.4, 1.0, &points);
        assert_eq!(slider.timing().beat_length, 500.0);
        assert_eq!(slider.timing().speed_multiplier, 4.0);
        close(slider.timing().duration, 250.0);
    }
}

#[test]
fn freezes_settings_at_start_and_new_red_resets_speed() {
    let objects = [slider(200.0, 280.0, 1, 1), slider(0.0, 280.0, 1, 0)];
    let points = [
        point(0.0, 500.0, true, 1),
        point(0.0, -50.0, false, 2),
        point(100.0, 400.0, true, 3),
    ];
    let sliders = linear_sliders(&objects, &difficulty(1.4, 1.0), &points).unwrap();
    assert_eq!(sliders[0].timing().object_id, ObjectId(0));
    assert_eq!(sliders[0].timing().speed_multiplier, 2.0);
    close(sliders[0].timing().end_time, 500.0);
    assert_eq!(sliders[1].timing().beat_length, 400.0);
    assert_eq!(sliders[1].timing().speed_multiplier, 1.0);
    close(sliders[1].timing().end_time, 1000.0);
}

#[test]
fn clamps_beat_length_and_speed_and_rounds_the_tempo_factor_to_f32() {
    let slow = prepared(
        100.0,
        1,
        1.0,
        1.0,
        &[
            point(0.0, 100_000.0, true, 1),
            point(0.0, -10_000.0, false, 2),
        ],
    );
    assert_eq!(slow.timing().beat_length, 60_000.0);
    assert_eq!(slow.timing().speed_multiplier, 0.1);
    close(slow.timing().span_duration, 600_000.0);
    let fast = prepared(
        100.0,
        1,
        1.0,
        1.0,
        &[point(0.0, 1.0, true, 1), point(0.0, -1.0, false, 2)],
    );
    assert_eq!(fast.timing().beat_length, 6.0);
    assert_eq!(fast.timing().speed_multiplier, 10.0);
    close(fast.timing().span_duration, 0.6);
    let rounded = prepared(
        350.0,
        1,
        1.4,
        1.0,
        &[
            point(0.0, 500.0, true, 1),
            point(0.0, -100.0 / 3.0, false, 2),
        ],
    );
    assert_eq!(rounded.timing().velocity, 0.8400000320434583);
    assert_ne!(rounded.timing().velocity, 0.84);
}

#[test]
fn nonnegative_inherited_values_reset_speed_through_the_parser() {
    for value in ["0", "-0", "50", "500"] {
        let text = format!(
            "osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n-100,500,4,0,0,100,1,0\n-50,-50,0,0,0,100,0,0\n0,{value},0,0,0,100,0,0\n[HitObjects]\n0,0,0,2,0,L|280:0,1,280"
        );
        let difficulty = osu_difficulty(text.as_bytes()).unwrap();
        let objects = osu_hit_objects(text.as_bytes()).unwrap();
        let points = osu_timing_points(text.as_bytes()).unwrap();
        let sliders = linear_sliders(&objects, &difficulty, &points).unwrap();
        assert_eq!(sliders[0].timing().speed_multiplier, 1.0);
        close(sliders[0].timing().duration, 1000.0);
    }
}

#[test]
fn tick_exclusion_is_ten_milliseconds_and_generation_length_is_capped() {
    for (length, count) in [(110.0, 0), (111.0, 1)] {
        let slider = prepared(length, 1, 1.0, 1.0, &[point(0.0, 100.0, true, 1)]);
        assert_eq!(
            slider
                .timing()
                .events
                .iter()
                .filter(|event| event.kind == SliderEventKind::Tick)
                .count(),
            count
        );
    }
    let long = prepared(200_000.0, 1, 1.0, 1.0, &[point(0.0, 100.0, true, 1)]);
    let ticks: Vec<_> = long
        .timing()
        .events
        .iter()
        .filter(|event| event.kind == SliderEventKind::Tick)
        .collect();
    assert_eq!(ticks.len(), 999);
    close(ticks[0].time, 200.0);
    assert_eq!(ticks[0].position, Point { x: 200.0, y: 0.0 });
}

#[test]
fn tail_check_respects_short_duration_and_the_last_part() {
    let short = prepared(40.0, 1, 1.0, 1.0, &[point(0.0, 100.0, true, 1)]);
    assert_eq!(short.timing().tail_check_time, 20.0);
    assert_eq!(short.timing().events[0].position, Point { x: 20.0, y: 0.0 });
    let dense = prepared(1000.0, 1, 1.0, 5.0, &[point(0.0, 100.0, true, 1)]);
    assert_eq!(dense.timing().tail_check_time, 980.0);
    let events = &dense.timing().events;
    assert_eq!(events[events.len() - 2].kind, SliderEventKind::Tick);
    assert_eq!(events[events.len() - 1].kind, SliderEventKind::TailCheck);
    assert_eq!(
        events[events.len() - 1].position,
        Point { x: 980.0, y: 0.0 }
    );
}

#[test]
fn zero_effective_length_keeps_repeats_and_tail_at_start() {
    let mut object = slider(-0.0, 100.0, 3, 0);
    if let HitObjectKind::Slider { controls, .. } = &mut object.kind {
        controls[1] = controls[0];
    }
    let sliders = linear_sliders(
        &[object],
        &difficulty(1.4, 1.0),
        &[point(0.0, 500.0, true, 1)],
    )
    .unwrap();
    let timing = sliders[0].timing();
    assert_eq!(sliders[0].path().length(), 0.0);
    assert_eq!(timing.duration, 0.0);
    assert_eq!(timing.end_time.to_bits(), 0.0_f64.to_bits());
    assert_eq!(timing.events.len(), 3);
    assert_eq!(
        timing
            .events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![
            SliderEventKind::Repeat,
            SliderEventKind::Repeat,
            SliderEventKind::TailCheck
        ]
    );
    assert!(timing.events.iter().all(|event| event.time == 0.0));
    assert_eq!(
        sliders[0].position_at_time(0.0).unwrap(),
        Point { x: 0.0, y: 0.0 }
    );
}

#[test]
fn rejects_missing_bpm_and_required_slider_settings() {
    for points in [
        vec![],
        vec![point(0.0, -50.0, false, 1)],
        vec![point(1.0, 500.0, true, 1)],
    ] {
        let result = linear_sliders(&[slider(0.0, 280.0, 1, 0)], &difficulty(1.4, 1.0), &points);
        assert!(matches!(
            result,
            Err(RoseaError::PreprocessError {
                object_id: ObjectId(0),
                line: 10,
                ..
            })
        ));
    }
    let mut missing = difficulty(1.4, 1.0);
    missing.slider_multiplier = None;
    assert_eq!(
        linear_sliders(&[slider(0.0, 280.0, 1, 0)], &missing, &[]).unwrap_err(),
        RoseaError::MissingDifficultyField("SliderMultiplier")
    );
    missing.slider_multiplier = Some(1.4);
    missing.slider_tick_rate = None;
    assert_eq!(
        linear_sliders(&[slider(0.0, 280.0, 1, 0)], &missing, &[]).unwrap_err(),
        RoseaError::MissingDifficultyField("SliderTickRate")
    );
    assert!(linear_sliders(&[], &missing, &[]).unwrap().is_empty());
}

#[test]
fn rejects_bad_inputs_and_numerical_overflow_or_underflow() {
    for (multiplier, rate, spans, length) in [
        (f64::MAX, 1.0, 1, 0.0),
        (1e-300, 1e100, 1, 0.0),
        (1e-305, 1.0, 2, 1000.0),
    ] {
        let result = linear_sliders(
            &[slider(0.0, length, spans, 0)],
            &difficulty(multiplier, rate),
            &[point(0.0, 100.0, true, 1)],
        );
        assert!(
            matches!(result, Err(RoseaError::NumericalError { .. })),
            "{multiplier}, {rate}"
        );
    }
    // При B=100 множитель сохраняется, а при B=500 скорость уже округляется в ноль.
    let smallest = prepared(
        0.0,
        1,
        f64::from_bits(1),
        1.0,
        &[point(0.0, 100.0, true, 1)],
    );
    assert_eq!(smallest.timing().velocity, f64::from_bits(1));
    assert!(matches!(
        linear_sliders(
            &[slider(0.0, 0.0, 1, 0)],
            &difficulty(f64::from_bits(1), 1.0),
            &[point(0.0, 500.0, true, 1)]
        ),
        Err(RoseaError::NumericalError { .. })
    ));
    for (multiplier, rate) in [
        (0.0, 1.0),
        (1.0, -1.0),
        (f64::NAN, 1.0),
        (1.0, f64::INFINITY),
    ] {
        assert!(matches!(
            linear_sliders(
                &[slider(0.0, 100.0, 1, 0)],
                &difficulty(multiplier, rate),
                &[]
            ),
            Err(RoseaError::PreprocessError { .. })
        ));
    }
    let bad_point = [point(0.0, f64::NAN, false, 3)];
    assert_eq!(
        linear_sliders(
            &[slider(0.0, 100.0, 1, 0)],
            &difficulty(1.4, 1.0),
            &bad_point
        )
        .unwrap_err(),
        RoseaError::InvalidTimingPointField {
            field: "beatLength",
            line: 3
        }
    );
    let slider = prepared(100.0, 1, 1.4, 1.0, &[point(0.0, 500.0, true, 1)]);
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            slider.position_at_time(time),
            Err(RoseaError::NumericalError { .. })
        ));
    }
}

#[test]
fn rejects_nonfinite_start_with_object_context() {
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            linear_sliders(&[slider(time, 100.0, 1, 2)], &difficulty(1.0, 1.0), &[]).unwrap_err(),
            RoseaError::NumericalError {
                line: 12,
                object_id: ObjectId(2),
            }
        );
    }
}

#[test]
fn bounds_end_time_and_pass_count() {
    let points = [point(0.0, 100.0, true, 1)];
    for (start, length) in [(86_400_000.25, 0.0), (86_399_990.0, 100.0)] {
        assert!(matches!(
            linear_sliders(
                &[slider(start, length, 1, 0)],
                &difficulty(1.0, 1.0),
                &points
            ),
            Err(RoseaError::ResourceLimit {
                resource: "absolute_time_ms",
                ..
            })
        ));
    }
    assert!(
        linear_sliders(
            &[slider(86_400_000.0, 0.0, 1, 0)],
            &difficulty(1.0, 1.0),
            &points
        )
        .is_ok()
    );
    assert!(matches!(
        linear_sliders(&[slider(0.0, 100.0, 0, 0)], &difficulty(1.0, 1.0), &points),
        Err(RoseaError::InvalidHitObjectField {
            field: "slides",
            ..
        })
    ));
    assert!(matches!(
        linear_sliders(
            &[slider(0.0, 100.0, 10_001, 0)],
            &difficulty(1.0, 1.0),
            &points
        ),
        Err(RoseaError::ResourceLimit {
            resource: "slider_spans",
            ..
        })
    ));
}

#[test]
fn bounds_total_events_and_vertices_across_sliders() {
    let points = [point(0.0, 100.0, true, 1)];
    let dense = [slider(0.0, 130.0, 1, 0), slider(0.0, 130.0, 1, 1)];
    assert_eq!(
        linear_sliders(&dense, &difficulty(1.0, 1_000_000.0), &points).unwrap_err(),
        RoseaError::ResourceLimit {
            resource: "slider_events",
            limit: 2_000_000,
            observed: 2_000_001
        }
    );
    let many_paths: Vec<_> = (0..489)
        .map(|id| {
            let mut object = slider(0.0, 4095.0, 1, id);
            if let HitObjectKind::Slider { controls, .. } = &mut object.kind {
                *controls = (0..4096)
                    .map(|x| Point {
                        x: x as f32,
                        y: 0.0,
                    })
                    .collect();
            }
            object
        })
        .collect();
    assert_eq!(
        linear_sliders(&many_paths, &difficulty(1.0, 1.0), &points).unwrap_err(),
        RoseaError::ResourceLimit {
            resource: "prepared_vertices",
            limit: 2_000_000,
            observed: 2_002_944
        }
    );
}
