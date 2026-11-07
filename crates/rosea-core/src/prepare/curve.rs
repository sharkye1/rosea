//! Геометрия L по SPEC §14.2: нормализация, префиксы расстояний и заданная длина.

use crate::error::RoseaError;
use crate::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};

const MAX_CONTROLS: usize = 4096;
const MAX_COORDINATE: f32 = 1_000_000.0;

/// Неизменяемый путь в координатах относительно нормализованного начала объекта.
/// Один и тот же путь предназначен для игровой логики и растеризатора.
#[derive(Debug)]
pub struct PreparedPath {
    origin: Point,
    vertices: Vec<Point>,
    cumulative_lengths: Vec<f64>,
    length: f64,
    object_id: ObjectId,
    line: usize,
}

impl PreparedPath {
    pub fn origin(&self) -> Point {
        self.origin
    }

    pub fn vertices(&self) -> &[Point] {
        &self.vertices
    }

    pub fn cumulative_lengths(&self) -> &[f64] {
        &self.cumulative_lengths
    }

    pub fn length(&self) -> f64 {
        self.length
    }

    /// Возвращает относительную позицию по доле пройденной длины; доля зажимается в [0,1].
    pub fn position_at(&self, progress: f64) -> Result<Point, RoseaError> {
        if !progress.is_finite() {
            return Err(self.numerical_error());
        }
        let progress = progress.clamp(0.0, 1.0);
        // Сохранённые концы возвращаем точно, без промежуточного умножения и интерполяции.
        if progress == 0.0 {
            return Ok(self.vertices[0]);
        }
        if progress == 1.0 {
            return Ok(self.vertices[self.vertices.len() - 1]);
        }
        self.position_at_distance(progress * self.length)
    }

    /// Выборка по расстоянию в osu!pixels; значения вне пути зажимаются к концам.
    pub fn position_at_distance(&self, distance: f64) -> Result<Point, RoseaError> {
        if !distance.is_finite() {
            return Err(self.numerical_error());
        }
        if self.length == 0.0 || distance <= 0.0 {
            return Ok(self.vertices[0]);
        }
        if distance >= self.length {
            return Ok(self.vertices[self.vertices.len() - 1]);
        }
        // Ищем первый префикс >= distance, чтобы равенство попадало в конец отрезка.
        let index = self
            .cumulative_lengths
            .partition_point(|&length| length < distance);
        let start = self.cumulative_lengths[index - 1];
        let span = self.cumulative_lengths[index] - start;
        if span <= 0.0 {
            return Err(self.numerical_error());
        }
        let point = interpolate(
            self.vertices[index - 1],
            self.vertices[index],
            (distance - start) / span,
        );
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(self.numerical_error());
        }
        Ok(point)
    }

    pub(super) fn numerical_error(&self) -> RoseaError {
        RoseaError::NumericalError {
            line: self.line,
            object_id: self.object_id,
        }
    }
}

/// Готовит геометрию одного L-слайдера по SPEC §14.2; времена и число проходов здесь не применяются.
pub fn linear_path(object: &HitObject) -> Result<PreparedPath, RoseaError> {
    let invalid = |field| RoseaError::InvalidHitObjectField {
        field,
        line: object.line,
        object_id: object.id,
    };
    let numerical = || RoseaError::NumericalError {
        line: object.line,
        object_id: object.id,
    };
    let HitObjectKind::Slider {
        kind: CurveKind::Linear,
        controls,
        declared_length,
        ..
    } = &object.kind
    else {
        return Err(invalid("type"));
    };
    if !declared_length.is_finite() || *declared_length < 0.0 {
        return Err(RoseaError::InvalidSliderLength {
            line: object.line,
            object_id: object.id,
        });
    }
    if controls.len() > MAX_CONTROLS {
        return Err(RoseaError::ResourceLimit {
            resource: "slider_control_points",
            limit: MAX_CONTROLS as u64,
            observed: controls.len() as u64,
        });
    }
    if controls.is_empty() {
        return Err(invalid("curvePoints"));
    }
    // Проверяем исходные координаты до clamp/trunc, чтобы нормализация не скрыла неверный вход.
    for point in controls {
        if !point.x.is_finite() || point.x.abs() > MAX_COORDINATE {
            return Err(invalid("x"));
        }
        if !point.y.is_finite() || point.y.abs() > MAX_COORDINATE {
            return Err(invalid("y"));
        }
    }
    if controls[0] != object.position {
        return Err(invalid("curvePoints"));
    }
    // y=512 намеренно: это граница legacy-нормализации, заданная в SPEC, а не высота поля.
    let origin = Point {
        x: object.position.x.clamp(0.0, 512.0).trunc() as i32 as f32,
        y: object.position.y.clamp(0.0, 512.0).trunc() as i32 as f32,
    };
    let mut vertices = Vec::with_capacity(controls.len() + 1);
    vertices.push(Point { x: 0.0, y: 0.0 });
    for point in &controls[1..] {
        vertices.push(Point {
            x: (point.x.trunc() as i32 as f32) - origin.x,
            y: (point.y.trunc() as i32 as f32) - origin.y,
        });
    }
    // Флаг сохраняется до удаления дублей: одинаковый конец запрещает удлинение.
    let duplicate_end =
        vertices.len() >= 2 && vertices[vertices.len() - 1] == vertices[vertices.len() - 2];
    let mut calculated_length = 0.0;
    for pair in vertices.windows(2) {
        calculated_length += segment_length(pair[0], pair[1]);
    }
    vertices.dedup();
    let mut cumulative_lengths = Vec::with_capacity(vertices.len() + 1);
    let mut prefix = 0.0;
    cumulative_lengths.push(prefix);
    for pair in vertices.windows(2) {
        prefix += segment_length(pair[0], pair[1]);
        cumulative_lengths.push(prefix);
    }

    let mut length = calculated_length;
    if *declared_length == 0.0 {
        vertices.truncate(1);
        cumulative_lengths.truncate(1);
        length = 0.0;
    } else if *declared_length < calculated_length {
        let index = cumulative_lengths.partition_point(|&value| value < *declared_length);
        let before = cumulative_lengths[index - 1];
        let fraction = (*declared_length - before) / (cumulative_lengths[index] - before);
        let end = interpolate(vertices[index - 1], vertices[index], fraction);
        if !end.x.is_finite() || !end.y.is_finite() {
            return Err(numerical());
        }
        vertices[index] = end;
        vertices.truncate(index + 1);
        cumulative_lengths[index] = *declared_length;
        cumulative_lengths.truncate(index + 1);
        length = *declared_length;
    } else if *declared_length > calculated_length && !duplicate_end && vertices.len() >= 2 {
        let last = vertices[vertices.len() - 1];
        let previous = vertices[vertices.len() - 2];
        let segment = segment_length(previous, last);
        let extra = *declared_length - calculated_length;
        let dx = f64::from(last.x) - f64::from(previous.x);
        let dy = f64::from(last.y) - f64::from(previous.y);
        let end = Point {
            x: (f64::from(last.x) + (dx / segment) * extra) as f32,
            y: (f64::from(last.y) + (dy / segment) * extra) as f32,
        };
        if !end.x.is_finite() || !end.y.is_finite() {
            return Err(numerical());
        }
        vertices.push(end);
        cumulative_lengths.push(*declared_length);
        length = *declared_length;
    }
    // Последний префикс после обрезки/удлинения уже равен объявленной длине.
    // Не пересчитываем его по округлённой f32-вершине.
    Ok(PreparedPath {
        origin,
        vertices,
        cumulative_lengths,
        length,
        object_id: object.id,
        line: object.line,
    })
}

fn segment_length(start: Point, end: Point) -> f64 {
    // Сначала расширение в f64, затем вычитание и квадратный корень в заданном порядке.
    let dx = f64::from(end.x) - f64::from(start.x);
    let dy = f64::from(end.y) - f64::from(start.y);
    (dx * dx + dy * dy).sqrt()
}

fn interpolate(start: Point, end: Point, fraction: f64) -> Point {
    Point {
        x: (f64::from(start.x) + (f64::from(end.x) - f64::from(start.x)) * fraction) as f32,
        y: (f64::from(start.y) + (f64::from(end.y) - f64::from(start.y)) * fraction) as f32,
    }
}
