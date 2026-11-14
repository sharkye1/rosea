//! Оценка времени нажатия относительно начала объекта.

use crate::model::{Judgement, Point, TimeMs};
use crate::prepare::HitWindows;

/// Возвращает оценку попадания по времени; клик вне окна не расходуется.
pub fn judge_click(windows: HitWindows, start: TimeMs, click_ms: i64) -> Option<Judgement> {
    let delta = (click_ms as f64 - start.get()).abs();
    if delta < windows.great as f64 {
        Some(Judgement::Great)
    } else if delta < windows.ok as f64 {
        Some(Judgement::Ok)
    } else if delta < windows.meh as f64 {
        Some(Judgement::Meh)
    } else {
        None
    }
}

/// Проверяет положение курсора в мировых координатах, включая границу круга.
pub fn inside_radius(cursor: Point, center: Point, radius: f32) -> bool {
    if !cursor.x.is_finite()
        || !cursor.y.is_finite()
        || !center.x.is_finite()
        || !center.y.is_finite()
        || !radius.is_finite()
        || radius < 0.0
    {
        return false;
    }
    let dx = f64::from(cursor.x) - f64::from(center.x);
    let dy = f64::from(cursor.y) - f64::from(center.y);
    let radius = f64::from(radius);
    dx * dx + dy * dy <= radius * radius
}
