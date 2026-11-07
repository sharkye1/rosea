//! Скорость слайдеров и времена вложенных частей. Арифметика сохраняет дробные миллисекунды.

use crate::error::RoseaError;
use crate::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point, TimingPoint};
use crate::parse::Difficulty;

use super::PreparedPath;
use super::curve::{PathBudget, path_with_budget};

const MAX_OBJECTS: usize = 100_000;
const MAX_TIMING_POINTS: usize = 100_000;
const MAX_EVENTS: usize = 2_000_000;
const MAX_SPANS: u32 = 10_000;
const MAX_TIME: f64 = 86_400_000.0;

/// Порядок вариантов задаёт порядок частей при одинаковом времени.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SliderEventKind {
    Tick,
    Repeat,
    TailCheck,
}

/// Позиция события относительна началу пути; span_index начинается с нуля.
#[derive(Debug, PartialEq)]
pub struct SliderEvent {
    pub time: f64,
    pub kind: SliderEventKind,
    pub span_index: u32,
    pub path_progress: f64,
    pub position: Point,
}

#[derive(Debug)]
pub struct SliderTiming {
    pub object_id: ObjectId,
    pub spans: u32,
    pub beat_length: f64,
    pub speed_multiplier: f64,
    pub velocity: f64,
    pub tick_distance: f64,
    pub span_duration: f64,
    pub duration: f64,
    pub start_time: f64,
    pub end_time: f64,
    pub tail_check_time: f64,
    pub end_position: Point,
    pub events: Vec<SliderEvent>,
}

/// Геометрия и времена хранятся вместе, чтобы вложенные части использовали тот же путь.
#[derive(Debug)]
pub struct PreparedSlider {
    path: PreparedPath,
    timing: SliderTiming,
}

impl PreparedSlider {
    pub(super) fn set_origin(&mut self, position: Point) {
        self.path.origin = position;
    }

    pub fn path(&self) -> &PreparedPath {
        &self.path
    }

    pub fn timing(&self) -> &SliderTiming {
        &self.timing
    }

    /// Позиция шарика относительно начала пути с учётом обратных проходов.
    pub fn position_at_time(&self, time: f64) -> Result<Point, RoseaError> {
        if !time.is_finite() {
            return Err(self.path.numerical_error());
        }
        if time < self.timing.start_time {
            return self.path.position_at(0.0);
        }
        if time > self.timing.end_time {
            return Ok(self.timing.end_position);
        }
        let (_, progress) = progress_at(
            time,
            self.timing.start_time,
            self.timing.span_duration,
            self.timing.spans,
        );
        self.path.position_at(progress)
    }
}

/// Готовит только L-слайдеры; смешанные L/B/P-карты готовятся через sliders.
pub fn linear_sliders(
    objects: &[HitObject],
    difficulty: &Difficulty,
    timing_points: &[TimingPoint],
) -> Result<Vec<PreparedSlider>, RoseaError> {
    for object in objects {
        if matches!(
            object.kind,
            HitObjectKind::Slider {
                kind: CurveKind::Bezier | CurveKind::PerfectCurve,
                ..
            }
        ) {
            return Err(RoseaError::InvalidHitObjectField {
                field: "type",
                line: object.line,
                object_id: object.id,
            });
        }
    }
    sliders(objects, difficulty, timing_points)
}

/// Готовит L/B/P-слайдеры карты, пропуская круги. Бюджеты геометрии и частей общие для вызова.
pub fn sliders(
    objects: &[HitObject],
    difficulty: &Difficulty,
    timing_points: &[TimingPoint],
) -> Result<Vec<PreparedSlider>, RoseaError> {
    for (resource, count, limit) in [
        ("hit_objects", objects.len(), MAX_OBJECTS),
        ("timing_points", timing_points.len(), MAX_TIMING_POINTS),
    ] {
        if count > limit {
            return Err(RoseaError::ResourceLimit {
                resource,
                limit: limit as u64,
                observed: count as u64,
            });
        }
    }
    let mut sliders: Vec<_> = objects
        .iter()
        .filter(|object| matches!(object.kind, HitObjectKind::Slider { .. }))
        .collect();
    for object in &sliders {
        checked_time(object.start_time, object)?;
    }
    sliders.sort_by(|a, b| {
        canonical_zero(a.start_time)
            .total_cmp(&canonical_zero(b.start_time))
            .then_with(|| a.id.0.cmp(&b.id.0))
    });
    if sliders.is_empty() {
        return Ok(Vec::new());
    }
    let multiplier = difficulty
        .slider_multiplier
        .ok_or(RoseaError::MissingDifficultyField("SliderMultiplier"))?;
    let tick_rate = difficulty
        .slider_tick_rate
        .ok_or(RoseaError::MissingDifficultyField("SliderTickRate"))?;
    for (field, value) in [
        ("SliderMultiplier", multiplier),
        ("SliderTickRate", tick_rate),
    ] {
        if !value.is_finite() || value <= 0.0 {
            let message = if field == "SliderMultiplier" {
                "SliderMultiplier должен быть конечным и строго положительным"
            } else {
                "SliderTickRate должен быть конечным и строго положительным"
            };
            return Err(RoseaError::PreprocessError {
                line: sliders[0].line,
                object_id: sliders[0].id,
                message,
            });
        }
    }
    for point in timing_points {
        if !point.time.is_finite() || point.time.abs() > MAX_TIME {
            return Err(RoseaError::InvalidTimingPointField {
                field: "time",
                line: point.line,
            });
        }
        if !point.beat_length.is_finite() || (point.uninherited && point.beat_length <= 0.0) {
            return Err(RoseaError::InvalidTimingPointField {
                field: "beatLength",
                line: point.line,
            });
        }
    }
    let mut points: Vec<_> = timing_points.iter().collect();
    points.sort_by(|a, b| {
        canonical_zero(a.time)
            .total_cmp(&canonical_zero(b.time))
            .then_with(|| a.line.cmp(&b.line))
    });
    let mut next_point = 0;
    let mut beat_length = None;
    let mut speed_multiplier = 1.0;
    let mut path_budget = PathBudget::default();
    let mut used_events = 0;
    let mut prepared = Vec::with_capacity(sliders.len());
    for object in sliders {
        // В группе берём первую красную точку и последнюю зелёную независимо от их чередования.
        while next_point < points.len() && points[next_point].time <= object.start_time {
            let time = points[next_point].time;
            let mut first_red = None;
            let mut last_green = None;
            while next_point < points.len() && points[next_point].time == time {
                let point = points[next_point];
                if point.uninherited {
                    if first_red.is_none() {
                        first_red = Some(point.beat_length.clamp(6.0, 60_000.0));
                    }
                } else {
                    last_green = Some(point.beat_length);
                }
                next_point += 1;
            }
            if let Some(value) = first_red {
                beat_length = Some(value);
                speed_multiplier = 1.0;
            }
            if let Some(value) = last_green {
                speed_multiplier = if value < 0.0 {
                    let raw = 100.0 / -value;
                    if !raw.is_finite() {
                        return Err(numerical(object));
                    }
                    raw.clamp(0.1, 10.0)
                } else {
                    1.0
                };
            }
        }
        let beat_length = beat_length.ok_or(RoseaError::PreprocessError {
            line: object.line,
            object_id: object.id,
            message: "До начала слайдера не задан BPM",
        })?;
        let path = path_with_budget(object, &mut path_budget)?;
        let timing = slider_timing(
            object,
            &path,
            multiplier,
            tick_rate,
            beat_length,
            speed_multiplier,
            &mut used_events,
        )?;
        prepared.push(PreparedSlider { path, timing });
    }
    Ok(prepared)
}

fn slider_timing(
    object: &HitObject,
    path: &PreparedPath,
    multiplier: f64,
    tick_rate: f64,
    beat_length: f64,
    speed_multiplier: f64,
    used_events: &mut usize,
) -> Result<SliderTiming, RoseaError> {
    let HitObjectKind::Slider { spans, .. } = object.kind else {
        return Err(numerical(object));
    };
    if spans == 0 {
        return Err(RoseaError::InvalidHitObjectField {
            field: "slides",
            line: object.line,
            object_id: object.id,
        });
    }
    if spans > MAX_SPANS {
        return Err(RoseaError::ResourceLimit {
            resource: "slider_spans",
            limit: u64::from(MAX_SPANS),
            observed: u64::from(spans),
        });
    }
    // Промежуточное округление к f32 существенно. Выражения скорости и расстояния не сокращаем.
    let raw_as_beat_length = 100.0 / speed_multiplier;
    let bpm_factor = f64::from((raw_as_beat_length as f32).clamp(10.0, 1000.0)) / 100.0;
    let adjusted_beat_length = beat_length * bpm_factor;
    let velocity = (100.0 * multiplier) / adjusted_beat_length;
    let scoring_distance = velocity * beat_length;
    let tick_distance = scoring_distance / tick_rate;
    if !velocity.is_finite()
        || velocity <= 0.0
        || !tick_distance.is_finite()
        || tick_distance <= 0.0
    {
        return Err(numerical(object));
    }
    let start_time = canonical_zero(object.start_time);
    let span_duration = path.length() / velocity;
    let duration = f64::from(spans) * span_duration;
    if !span_duration.is_finite() || !duration.is_finite() {
        return Err(numerical(object));
    }
    let end_time = checked_time(start_time + duration, object)?;
    let mut events = Vec::new();
    let mut last_part_time = start_time;
    let generation_length = path.length().min(100_000.0);
    let step = tick_distance.min(generation_length);
    let exclusion_distance = if generation_length == 0.0 {
        0.0
    } else {
        velocity * 10.0
    };
    if !exclusion_distance.is_finite() {
        return Err(numerical(object));
    }
    for span_index in 0..spans {
        if generation_length > 0.0 {
            let mut distance = step;
            while distance <= generation_length {
                // Оставляем десять миллисекунд до края, а не десять пикселей.
                if distance >= generation_length - exclusion_distance {
                    break;
                }
                let progress = distance / generation_length;
                let time_progress = if span_index % 2 == 0 {
                    progress
                } else {
                    1.0 - progress
                };
                let time = checked_time(
                    start_time
                        + f64::from(span_index) * span_duration
                        + time_progress * span_duration,
                    object,
                )?;
                last_part_time = last_part_time.max(time);
                push_event(
                    &mut events,
                    used_events,
                    SliderEvent {
                        time,
                        kind: SliderEventKind::Tick,
                        span_index,
                        path_progress: progress,
                        position: path.position_at(progress)?,
                    },
                )?;
                let next = distance + step;
                if next <= distance {
                    return Err(numerical(object));
                }
                distance = next;
            }
        }
        if span_index + 1 < spans {
            let time = checked_time(
                start_time + f64::from(span_index + 1) * span_duration,
                object,
            )?;
            let progress = if span_index % 2 == 0 { 1.0 } else { 0.0 };
            last_part_time = last_part_time.max(time);
            push_event(
                &mut events,
                used_events,
                SliderEvent {
                    time,
                    kind: SliderEventKind::Repeat,
                    span_index,
                    path_progress: progress,
                    position: path.position_at(progress)?,
                },
            )?;
        }
    }
    let tail_check_time = checked_time(
        (start_time + duration / 2.0)
            .max(end_time - 36.0)
            .max(last_part_time),
        object,
    )?;
    let (tail_span, tail_progress) = progress_at(tail_check_time, start_time, span_duration, spans);
    push_event(
        &mut events,
        used_events,
        SliderEvent {
            time: tail_check_time,
            kind: SliderEventKind::TailCheck,
            span_index: tail_span,
            path_progress: tail_progress,
            position: path.position_at(tail_progress)?,
        },
    )?;
    // Обратный проход меняет порядок тиков по времени; равные времена сохраняют порядок фаз.
    events.sort_by(|a, b| {
        a.time
            .total_cmp(&b.time)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.span_index.cmp(&b.span_index))
    });
    let end_progress = if spans % 2 == 1 { 1.0 } else { 0.0 };
    Ok(SliderTiming {
        object_id: object.id,
        spans,
        beat_length,
        speed_multiplier,
        velocity,
        tick_distance,
        span_duration,
        duration,
        start_time,
        end_time,
        tail_check_time,
        end_position: path.position_at(end_progress)?,
        events,
    })
}

fn progress_at(time: f64, start: f64, span_duration: f64, spans: u32) -> (u32, f64) {
    if span_duration == 0.0 {
        return (spans - 1, if spans % 2 == 1 { 1.0 } else { 0.0 });
    }
    let q = ((time - start) / span_duration).clamp(0.0, f64::from(spans));
    let (span, local) = if q == f64::from(spans) {
        (spans - 1, 1.0)
    } else {
        let span = q.floor() as u32;
        (span, q - f64::from(span))
    };
    (span, if span % 2 == 0 { local } else { 1.0 - local })
}

fn push_event(
    events: &mut Vec<SliderEvent>,
    used: &mut usize,
    event: SliderEvent,
) -> Result<(), RoseaError> {
    if *used == MAX_EVENTS {
        return Err(RoseaError::ResourceLimit {
            resource: "slider_events",
            limit: MAX_EVENTS as u64,
            observed: *used as u64 + 1,
        });
    }
    *used += 1;
    events.push(event);
    Ok(())
}

fn checked_time(time: f64, object: &HitObject) -> Result<f64, RoseaError> {
    if !time.is_finite() {
        return Err(numerical(object));
    }
    if time.abs() > MAX_TIME {
        // В диагностике целого лимита округляем вверх; само время события не изменяем.
        return Err(RoseaError::ResourceLimit {
            resource: "absolute_time_ms",
            limit: MAX_TIME as u64,
            observed: time.abs().ceil() as u64,
        });
    }
    Ok(canonical_zero(time))
}

fn numerical(object: &HitObject) -> RoseaError {
    RoseaError::NumericalError {
        line: object.line,
        object_id: object.id,
    }
}

fn canonical_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}
