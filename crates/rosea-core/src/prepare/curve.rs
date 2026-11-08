//! Геометрия L/B/P: нормализация, аппроксимация кривых и выборка по длине пути.

use crate::error::RoseaError;
use crate::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};

const MAX_CONTROLS: usize = 4096;
const MAX_COORDINATE: f32 = 1_000_000.0;
const MAX_VERTICES: usize = 2_000_000;
const MAX_INTERPOLATIONS: usize = 20_000_000;
const MAX_DEPTH: u32 = 32;

/// Счётчики общие для всех путей одного вызова подготовки карты.
#[derive(Default)]
pub(super) struct PathBudget {
    vertices: usize,
    interpolations: usize,
}

impl PathBudget {
    fn check_vertices(&self, count: usize) -> Result<(), RoseaError> {
        if count > MAX_VERTICES - self.vertices {
            return Err(RoseaError::ResourceLimit {
                resource: "prepared_vertices",
                limit: MAX_VERTICES as u64,
                observed: (self.vertices + count) as u64,
            });
        }
        Ok(())
    }

    fn reserve_interpolations(&mut self, count: usize) -> Result<(), RoseaError> {
        if count > MAX_INTERPOLATIONS - self.interpolations {
            return Err(RoseaError::ResourceLimit {
                resource: "bezier_interpolations",
                limit: MAX_INTERPOLATIONS as u64,
                observed: (self.interpolations + count) as u64,
            });
        }
        self.interpolations += count;
        Ok(())
    }
}

/// Неизменяемый путь в координатах относительно нормализованного начала объекта.
/// Один и тот же путь предназначен для игровой логики и растеризатора.
#[derive(Debug)]
pub struct PreparedPath {
    pub(super) origin: Point,
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

    /// Возвращает относительную позицию по доле пройденной длины; доля зажимается в `[0, 1]`.
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

/// Готовит только L-путь; для L/B/P используется slider_path.
pub fn linear_path(object: &HitObject) -> Result<PreparedPath, RoseaError> {
    if !matches!(
        object.kind,
        HitObjectKind::Slider {
            kind: CurveKind::Linear,
            ..
        }
    ) {
        return Err(RoseaError::InvalidHitObjectField {
            field: "type",
            line: object.line,
            object_id: object.id,
        });
    }
    slider_path(object)
}

/// Готовит один L/B/P-путь. Времена и число проходов здесь не применяются.
pub fn slider_path(object: &HitObject) -> Result<PreparedPath, RoseaError> {
    path_with_budget(object, &mut PathBudget::default())
}

pub(super) fn path_with_budget(
    object: &HitObject,
    budget: &mut PathBudget,
) -> Result<PreparedPath, RoseaError> {
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
        kind,
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
    // y=512 — граница legacy-нормализации исходных координат.
    let origin = Point {
        x: object.position.x.clamp(0.0, 512.0).trunc() as i32 as f32,
        y: object.position.y.clamp(0.0, 512.0).trunc() as i32 as f32,
    };
    if *kind == CurveKind::Linear {
        budget.check_vertices(controls.len())?;
    }
    let mut vertices = Vec::with_capacity(controls.len() + 1);
    vertices.push(Point { x: 0.0, y: 0.0 });
    for point in &controls[1..] {
        vertices.push(Point {
            x: (point.x.trunc() as i32 as f32) - origin.x,
            y: (point.y.trunc() as i32 as f32) - origin.y,
        });
    }
    vertices = match kind {
        CurveKind::Linear => vertices,
        CurveKind::Bezier => bezier_path(&vertices, budget)?,
        CurveKind::PerfectCurve => perfect_path(vertices, object, budget)?,
    };
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
        budget.check_vertices(vertices.len() + 1)?;
        vertices.push(end);
        cumulative_lengths.push(*declared_length);
        length = *declared_length;
    }
    // Последний префикс после обрезки/удлинения уже равен объявленной длине.
    // Не пересчитываем его по округлённой f32-вершине.
    budget.vertices += vertices.len();
    Ok(PreparedPath {
        origin,
        vertices,
        cumulative_lengths,
        length,
        object_id: object.id,
        line: object.line,
    })
}

fn perfect_path(
    controls: Vec<Point>,
    object: &HitObject,
    budget: &mut PathBudget,
) -> Result<Vec<Point>, RoseaError> {
    if controls.len() != 3 {
        return bezier_path(&controls, budget);
    }
    let [a, b, c] = [controls[0], controls[1], controls[2]]
        .map(|point| [f64::from(point.x), f64::from(point.y)]);
    let determinant = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    // При точной коллинеарности оставляем ломаную через B, включая обратное движение.
    if determinant == 0.0 {
        budget.check_vertices(controls.len())?;
        return Ok(controls);
    }
    let za = a[0] * a[0] + a[1] * a[1];
    let zb = b[0] * b[0] + b[1] * b[1];
    let zc = c[0] * c[0] + c[1] * c[1];
    let ox = (za * (b[1] - c[1]) + zb * (c[1] - a[1]) + zc * (a[1] - b[1])) / determinant;
    let oy = (za * (c[0] - b[0]) + zb * (a[0] - c[0]) + zc * (b[0] - a[0])) / determinant;
    let dx = a[0] - ox;
    let dy = a[1] - oy;
    let radius = (dx * dx + dy * dy).sqrt();
    let numerical = || RoseaError::NumericalError {
        line: object.line,
        object_id: object.id,
    };
    if !ox.is_finite() || !oy.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return Err(numerical());
    }
    let start_angle = (a[1] - oy).atan2(a[0] - ox);
    let middle_angle = (b[1] - oy).atan2(b[0] - ox);
    let end_angle = (c[1] - oy).atan2(c[0] - ox);
    let mod2pi = |angle: f64| {
        let value = angle.rem_euclid(std::f64::consts::TAU);
        // Для очень малого отрицательного угла rem_euclid может округлиться к самому TAU.
        if value == std::f64::consts::TAU {
            0.0
        } else {
            value
        }
    };
    let ac = mod2pi(end_angle - start_angle);
    let ab = mod2pi(middle_angle - start_angle);
    // Выбираем направление через среднюю точку, даже когда это большая дуга.
    let sweep = if ab <= ac {
        ac
    } else {
        ac - std::f64::consts::TAU
    };
    let max_angle = 2.0 * (1.0 - 0.1 / radius).clamp(-1.0, 1.0).acos();
    if max_angle == 0.0 {
        // Угловой шаг не представим: конечного числа вершин для допуска уже недостаточно.
        return Err(RoseaError::ResourceLimit {
            resource: "prepared_vertices",
            limit: MAX_VERTICES as u64,
            observed: u64::MAX,
        });
    }
    let segments = (sweep.abs() / max_angle).ceil().max(1.0);
    if !segments.is_finite() {
        return Err(numerical());
    }
    let count = segments + 1.0;
    if count > (MAX_VERTICES - budget.vertices) as f64 {
        return Err(RoseaError::ResourceLimit {
            resource: "prepared_vertices",
            limit: MAX_VERTICES as u64,
            observed: (budget.vertices as f64 + count) as u64,
        });
    }
    // Число сегментов проверено до преобразования и выделения всей ломаной.
    let segments = segments as usize;
    let mut vertices = Vec::with_capacity(segments + 1);
    for index in 0..=segments {
        let angle = start_angle + sweep * (index as f64 / segments as f64);
        let point = Point {
            x: (ox + radius * angle.cos()) as f32,
            y: (oy + radius * angle.sin()) as f32,
        };
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(numerical());
        }
        vertices.push(point);
    }
    // Концы берём из исходных точек, чтобы тригонометрия не сместила их округлением.
    vertices[0] = controls[0];
    vertices[segments] = controls[2];
    Ok(vertices)
}

fn bezier_path(controls: &[Point], budget: &mut PathBudget) -> Result<Vec<Point>, RoseaError> {
    let mut vertices = Vec::new();
    let mut start = 0;
    for index in 0..controls.len() {
        if index + 1 == controls.len() || controls[index] == controls[index + 1] {
            let segment: Vec<_> = controls[start..=index]
                .iter()
                .map(|point| [f64::from(point.x), f64::from(point.y)])
                .collect();
            approximate_bezier(&segment, 0, &mut vertices, budget)?;
            start = index + 1;
        }
    }
    Ok(vertices)
}

fn approximate_bezier(
    controls: &[[f64; 2]],
    depth: u32,
    vertices: &mut Vec<Point>,
    budget: &mut PathBudget,
) -> Result<(), RoseaError> {
    let degree = controls.len() - 1;
    let first = controls[0];
    let last = controls[degree];
    // Сравниваем с равномерной параметризацией хорды: одного расстояния до прямой мало.
    let flat = degree == 0
        || controls.iter().enumerate().all(|(index, point)| {
            let fraction = index as f64 / degree as f64;
            let dx = point[0] - (first[0] + (last[0] - first[0]) * fraction);
            let dy = point[1] - (first[1] + (last[1] - first[1]) * fraction);
            dx * dx + dy * dy <= 0.25 * 0.25
        });
    if flat {
        for value in [first, last] {
            let point = Point {
                x: value[0] as f32,
                y: value[1] as f32,
            };
            if vertices.last() != Some(&point) {
                budget.check_vertices(vertices.len() + 1)?;
                vertices.push(point);
            }
        }
        return Ok(());
    }
    if depth == MAX_DEPTH {
        return Err(RoseaError::ResourceLimit {
            resource: "bezier_depth",
            limit: u64::from(MAX_DEPTH),
            observed: u64::from(depth) + 1,
        });
    }
    // Полное число соседних средних известно заранее; проверяем бюджет до выделения дочерних наборов.
    budget.reserve_interpolations(degree * (degree + 1) / 2)?;
    let mut level = controls.to_vec();
    let mut left = Vec::with_capacity(controls.len());
    let mut right = Vec::with_capacity(controls.len());
    left.push(first);
    right.push(last);
    for size in (1..controls.len()).rev() {
        for index in 0..size {
            level[index] = [
                (level[index][0] + level[index + 1][0]) / 2.0,
                (level[index][1] + level[index + 1][1]) / 2.0,
            ];
        }
        left.push(level[0]);
        right.push(level[size - 1]);
    }
    right.reverse();
    approximate_bezier(&left, depth + 1, vertices, budget)?;
    approximate_bezier(&right, depth + 1, vertices, budget)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_curve_checks_remaining_vertices_and_shares_bezier_work_budget() {
        let mut object = HitObject {
            id: ObjectId(0),
            line: 1,
            position: Point { x: 0.0, y: 0.0 },
            start_time: 0.0,
            flags: 2,
            hit_sound: 0,
            kind: HitObjectKind::Slider {
                kind: CurveKind::PerfectCurve,
                controls: vec![
                    Point { x: 0.0, y: 0.0 },
                    Point { x: 100.0, y: 100.0 },
                    Point { x: 200.0, y: 0.0 },
                ],
                spans: 1,
                declared_length: 0.0,
            },
        };
        let mut budget = PathBudget {
            vertices: MAX_VERTICES - 36,
            interpolations: 0,
        };
        assert_eq!(
            path_with_budget(&object, &mut budget).unwrap_err(),
            RoseaError::ResourceLimit {
                resource: "prepared_vertices",
                limit: 2_000_000,
                observed: 2_000_001,
            }
        );
        assert_eq!(budget.vertices, MAX_VERTICES - 36);
        if let HitObjectKind::Slider { controls, .. } = &mut object.kind {
            controls.push(Point { x: 200.0, y: 0.0 });
        }
        budget = PathBudget {
            vertices: 0,
            interpolations: MAX_INTERPOLATIONS,
        };
        assert_eq!(
            path_with_budget(&object, &mut budget).unwrap_err(),
            RoseaError::ResourceLimit {
                resource: "bezier_interpolations",
                limit: 20_000_000,
                observed: 20_000_003,
            }
        );
        assert_eq!(budget.interpolations, MAX_INTERPOLATIONS);
        assert_eq!(budget.vertices, 0);
    }

    #[test]
    fn flatness_includes_the_boundary_and_subdivides_above_it() {
        for (height, expected_count) in [(0.25, 2), (0.250001, 3)] {
            let mut vertices = Vec::new();
            approximate_bezier(
                &[[0.0, 0.0], [0.5, height], [1.0, 0.0]],
                0,
                &mut vertices,
                &mut PathBudget::default(),
            )
            .unwrap();
            assert_eq!(vertices.len(), expected_count);
        }
    }

    #[test]
    fn depth_limit_rejects_unfinished_curve_but_accepts_a_flat_leaf() {
        let mut vertices = Vec::new();
        let mut budget = PathBudget::default();
        assert_eq!(
            approximate_bezier(
                &[[0.0, 0.0], [50.0, 100.0], [100.0, 0.0]],
                MAX_DEPTH,
                &mut vertices,
                &mut budget
            ),
            Err(RoseaError::ResourceLimit {
                resource: "bezier_depth",
                limit: 32,
                observed: 33,
            })
        );
        assert!(vertices.is_empty());
        assert_eq!(budget.interpolations, 0);
        approximate_bezier(
            &[[0.0, 0.0], [100.0, 0.0]],
            MAX_DEPTH,
            &mut vertices,
            &mut budget,
        )
        .unwrap();
        assert_eq!(vertices.len(), 2);
    }

    #[test]
    fn exhausted_budgets_fail_before_emitting_vertices_or_splitting() {
        let mut vertices = Vec::new();
        let mut budget = PathBudget {
            vertices: MAX_VERTICES,
            interpolations: 0,
        };
        assert_eq!(
            approximate_bezier(&[[0.0, 0.0]], 0, &mut vertices, &mut budget),
            Err(RoseaError::ResourceLimit {
                resource: "prepared_vertices",
                limit: 2_000_000,
                observed: 2_000_001,
            })
        );
        assert!(vertices.is_empty());
        budget = PathBudget {
            vertices: 0,
            interpolations: MAX_INTERPOLATIONS - 3,
        };
        // Один вызов тратит последние три операции; следующий уже не должен менять счётчик.
        let controls = [[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]];
        approximate_bezier(&controls, 0, &mut vertices, &mut budget).unwrap();
        assert_eq!(budget.interpolations, MAX_INTERPOLATIONS);
        let before = vertices.clone();
        assert_eq!(
            approximate_bezier(&controls, 0, &mut vertices, &mut budget),
            Err(RoseaError::ResourceLimit {
                resource: "bezier_interpolations",
                limit: 20_000_000,
                observed: 20_000_003,
            })
        );
        assert_eq!(vertices, before);
        assert_eq!(budget.interpolations, MAX_INTERPOLATIONS);
    }
}
