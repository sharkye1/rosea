//! Совместимый вход для карт только с кругами; общий разбор находится в hit_objects.

use crate::error::RoseaError;
use crate::model::Circle;

use super::hit_objects::parse_objects;

/// Читает карту с кругами; другие типы объектов возвращают ошибку.
pub fn osu_circles(bytes: &[u8]) -> Result<Vec<Circle>, RoseaError> {
    // Запрет слайдеров проверяется до разбора их полей, как у прежнего входа.
    let objects = parse_objects(bytes, false)?;
    Ok(objects
        .into_iter()
        .map(|object| Circle {
            id: object.id,
            line: object.line,
            position: object.position,
            start_time: object.start_time,
            flags: object.flags,
            hit_sound: object.hit_sound,
        })
        .collect())
}
