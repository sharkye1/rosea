//! Stacking v14 по несмещённым началам и концам; геометрия путей остаётся относительной.

use crate::error::RoseaError;
use crate::model::{HitObject, HitObjectKind, ObjectId, Point, TimingPoint};
use crate::parse::Difficulty;

use super::{PreparedSlider, sliders};

const MAX_OBJECTS: usize = 100_000;
const MAX_CHECKS: usize = 20_000_000;
const MAX_TIME: f64 = 86_400_000.0;

/// Объект после stacking. Позиции мировые, вершины пути и позиции его частей — относительные.
#[derive(Debug)]
pub struct StackedObject {
    object_id: ObjectId,
    line: usize,
    height: i32,
    offset: Point,
    position: Point,
    slider: Option<PreparedSlider>,
}

impl StackedObject {
    pub fn object_id(&self) -> ObjectId {
        self.object_id
    }
    pub fn height(&self) -> i32 {
        self.height
    }
    pub fn offset(&self) -> Point {
        self.offset
    }
    pub fn position(&self) -> Point {
        self.position
    }
    pub fn slider(&self) -> Option<&PreparedSlider> {
        self.slider.as_ref()
    }

    pub fn end_position(&self) -> Point {
        match &self.slider {
            Some(slider) => add(self.position, slider.timing().end_position),
            None => self.position,
        }
    }

    /// Мировая позиция круга или шарика слайдера с учётом stacking.
    pub fn position_at_time(&self, time: f64) -> Result<Point, RoseaError> {
        if !time.is_finite() {
            return Err(self.numerical_error());
        }
        let position = match &self.slider {
            Some(slider) => add(self.position, slider.position_at_time(time)?),
            None => self.position,
        };
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(self.numerical_error());
        }
        Ok(position)
    }

    fn numerical_error(&self) -> RoseaError {
        RoseaError::NumericalError {
            line: self.line,
            object_id: self.object_id,
        }
    }
}

/// Готовит круги и L/B/P-слайдеры по (start_time, ObjectId), затем применяет stacking.
pub fn stack_objects(
    objects: &[HitObject],
    difficulty: &Difficulty,
    timing_points: &[TimingPoint],
    stack_leniency: f32,
) -> Result<Vec<StackedObject>, RoseaError> {
    if objects.len() > MAX_OBJECTS {
        return Err(RoseaError::ResourceLimit {
            resource: "hit_objects",
            limit: MAX_OBJECTS as u64,
            observed: objects.len() as u64,
        });
    }
    if objects.is_empty() {
        return Ok(Vec::new());
    }
    for (value, valid, message) in [
        (
            difficulty.cs,
            (0.0..=10.0).contains(&difficulty.cs),
            "CircleSize должен быть конечным числом от 0 до 10",
        ),
        (
            difficulty.ar,
            (0.0..=10.0).contains(&difficulty.ar),
            "ApproachRate должен быть конечным числом от 0 до 10",
        ),
        (
            f64::from(stack_leniency),
            (0.0..=1.0).contains(&stack_leniency),
            "StackLeniency должен быть конечным числом от 0 до 1",
        ),
    ] {
        if !value.is_finite() || !valid {
            return Err(RoseaError::PreprocessError {
                line: objects[0].line,
                object_id: objects[0].id,
                message,
            });
        }
    }
    for object in objects {
        if !object.start_time.is_finite() {
            return Err(RoseaError::InvalidHitObjectField {
                field: "time",
                line: object.line,
                object_id: object.id,
            });
        }
        if object.start_time.abs() > MAX_TIME {
            return Err(RoseaError::ResourceLimit {
                resource: "absolute_time_ms",
                limit: MAX_TIME as u64,
                observed: object.start_time.abs().ceil() as u64,
            });
        }
        for (field, value) in [("x", object.position.x), ("y", object.position.y)] {
            if !value.is_finite() || value.abs() > 1_000_000.0 {
                return Err(RoseaError::InvalidHitObjectField {
                    field,
                    line: object.line,
                    object_id: object.id,
                });
            }
        }
    }
    let (threshold, scale) =
        stack_parameters(difficulty.cs as f32, difficulty.ar as f32, stack_leniency);
    let mut order: Vec<_> = objects.iter().collect();
    order.sort_by(|a, b| {
        // Оба написания нуля задают одно время; при равенстве сравниваем исходные ID.
        if a.start_time == b.start_time {
            a.id.0.cmp(&b.id.0)
        } else {
            a.start_time.total_cmp(&b.start_time)
        }
    });
    let mut prepared_sliders = sliders(objects, difficulty, timing_points)?.into_iter();
    let mut result = Vec::with_capacity(objects.len());
    for object in &order {
        let slider = if matches!(object.kind, HitObjectKind::Slider { .. }) {
            Some(prepared_sliders.next().ok_or(RoseaError::PreprocessError {
                line: object.line,
                object_id: object.id,
                message: "Не найден подготовленный слайдер",
            })?)
        } else {
            None
        };
        let position = match &slider {
            Some(slider) => slider.path().origin(),
            None => Point {
                x: object.position.x.clamp(0.0, 512.0).trunc() as i32 as f32,
                y: object.position.y.clamp(0.0, 512.0).trunc() as i32 as f32,
            },
        };
        let prepared = StackedObject {
            object_id: object.id,
            line: object.line,
            height: 0,
            offset: Point { x: 0.0, y: 0.0 },
            position,
            slider,
        };
        let end = prepared.end_position();
        if !end.x.is_finite() || !end.y.is_finite() {
            return Err(prepared.numerical_error());
        }
        result.push(prepared);
    }
    let mut checks = 0;
    for i in (1..result.len()).rev() {
        if result[i].height != 0 {
            continue;
        }
        let circle = result[i].slider.is_none();
        let mut current = i;
        for n in (0..i).rev() {
            let gap = if circle {
                let end_time = result[n]
                    .slider
                    .as_ref()
                    .map_or(order[n].start_time, |s| s.timing().end_time);
                order[current].start_time.trunc() - end_time.trunc()
            } else {
                order[current].start_time - order[n].start_time
            };
            if gap > f64::from(threshold) {
                break;
            }
            if circle
                && result[n].slider.is_some()
                && near(
                    result[n].end_position(),
                    result[current].position,
                    &mut checks,
                    &result[n],
                )?
            {
                let decrease = result[current]
                    .height
                    .checked_sub(result[n].height)
                    .and_then(|value| value.checked_add(1))
                    .ok_or_else(|| result[i].numerical_error())?;
                for j in n + 1..=i {
                    if near(
                        result[n].end_position(),
                        result[j].position,
                        &mut checks,
                        &result[j],
                    )? {
                        result[j].height = result[j]
                            .height
                            .checked_sub(decrease)
                            .ok_or_else(|| result[j].numerical_error())?;
                    }
                }
                break;
            }
            let previous = if circle {
                result[n].position
            } else {
                result[n].end_position()
            };
            if near(previous, result[current].position, &mut checks, &result[n])? {
                result[n].height = result[current]
                    .height
                    .checked_add(1)
                    .ok_or_else(|| result[n].numerical_error())?;
                current = n;
            }
        }
    }
    // Смещаем только после полного прохода: сравнения выше всегда используют исходные позиции.
    for object in &mut result {
        let offset = (object.height as f32 * scale) * -6.4_f32;
        object.offset = Point {
            x: offset,
            y: offset,
        };
        object.position = add(object.position, object.offset);
        if let Some(slider) = &mut object.slider {
            slider.set_origin(object.position);
        }
    }
    Ok(result)
}

fn stack_parameters(cs: f32, ar: f32, leniency: f32) -> (f32, f32) {
    // Стадии f64/f32 и целый preempt существенны для граничных расстояний и времён.
    let u = (f64::from(cs) - 5.0) / 5.0;
    let q = (1.0 - f64::from(0.7_f32) * u) as f32;
    let scale = (q / 2.0_f32) * 1.00041_f32;
    let ar = f64::from(ar);
    let preempt = if ar < 5.0 {
        (1200.0 + 120.0 * (5.0 - ar)).floor()
    } else {
        (1200.0 - 150.0 * (ar - 5.0)).floor()
    };
    ((preempt as f32) * leniency, scale)
}

fn near(a: Point, b: Point, used: &mut usize, object: &StackedObject) -> Result<bool, RoseaError> {
    if *used == MAX_CHECKS {
        return Err(RoseaError::ResourceLimit {
            resource: "stacking_checks",
            limit: MAX_CHECKS as u64,
            observed: *used as u64 + 1,
        });
    }
    *used += 1;
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let squared = dx * dx + dy * dy;
    if !squared.is_finite() {
        return Err(object.numerical_error());
    }
    Ok(squared.sqrt() < 3.0_f32)
}

fn add(a: Point, b: Point) -> Point {
    Point {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_and_preempt_keep_the_required_rounding_stages() {
        for (cs, expected_radius) in [
            (0.0, 54.422_302_246_093_75),
            (5.0, 32.013_118_743_896_484),
            (10.0, 9.603_936_195_373_535),
        ] {
            let (_, scale) = stack_parameters(cs, 5.0, 0.7);
            assert!((f64::from(64.0_f32 * scale) - expected_radius).abs() <= 1e-8);
        }
        for (ar, expected) in [
            (0.0, 1800.0),
            (5.0, 1200.0),
            (10.0, 450.0),
            (4.99, 1201.0),
            (5.01, 1198.0),
        ] {
            assert_eq!(stack_parameters(5.0, ar, 1.0).0, expected);
        }
    }

    #[test]
    fn near_uses_f32_distance_and_checks_the_budget_before_calculating() {
        let object = StackedObject {
            object_id: ObjectId(2),
            line: 9,
            height: 0,
            position: Point { x: 0.0, y: 0.0 },
            offset: Point { x: 0.0, y: 0.0 },
            slider: None,
        };
        let diagonal = Point {
            x: 2.5,
            y: 1.658_312_3,
        };
        let precise_squared = f64::from(diagonal.x) * f64::from(diagonal.x)
            + f64::from(diagonal.y) * f64::from(diagonal.y);
        assert!(precise_squared < 9.0);
        let mut used = 0;
        // В f32 квадрат расстояния округляется к 9, поэтому точка не считается близкой.
        assert!(!near(object.position, diagonal, &mut used, &object).unwrap());
        assert!(
            near(
                object.position,
                Point {
                    x: 3.0_f32.next_down(),
                    y: 0.0
                },
                &mut used,
                &object
            )
            .unwrap()
        );
        used = MAX_CHECKS - 1;
        assert!(
            !near(
                object.position,
                Point { x: 3.0, y: 0.0 },
                &mut used,
                &object
            )
            .unwrap()
        );
        assert_eq!(used, MAX_CHECKS);
        assert_eq!(
            near(
                object.position,
                Point {
                    x: f32::NAN,
                    y: 0.0
                },
                &mut used,
                &object
            ),
            Err(RoseaError::ResourceLimit {
                resource: "stacking_checks",
                limit: 20_000_000,
                observed: 20_000_001,
            })
        );
        assert_eq!(used, MAX_CHECKS);
    }
}
