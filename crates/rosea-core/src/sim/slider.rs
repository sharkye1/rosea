//! Проверка удержания шарика слайдера.

use crate::model::Point;
use crate::replay::ButtonState;

use super::hit::inside_radius;

/// Возвращает tracking после одной выборки курсора и кнопок.
pub fn tracking_at_sample(
    tracking: bool,
    buttons: ButtonState,
    cursor: Point,
    ball: Point,
    radius: f32,
) -> bool {
    if !buttons.left && !buttons.right {
        return false;
    }
    let threshold = if tracking { radius * 2.4_f32 } else { radius };
    inside_radius(cursor, ball, threshold)
}
