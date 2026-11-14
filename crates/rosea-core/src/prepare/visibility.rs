//! Размер объектов и время их появления в логических координатах.

use crate::error::RoseaError;
use crate::model::{Beatmap, HitObjectKind, TimeMs};

/// Параметры видимости, вычисленные по сложности и объектам карты.
#[derive(Debug, PartialEq)]
pub struct Visibility {
    pub radius: f32,
    pub preempt: f64,
    pub fade_in: f64,
    pub first_visible_at: TimeMs,
}

/// Готовит размеры и момент появления первого объекта карты.
pub fn visibility(map: &Beatmap) -> Result<Visibility, RoseaError> {
    let (scale, preempt) = scale_and_preempt(map.difficulty().cs, map.difficulty().ar);
    let mut first = None;
    for object in map.objects() {
        let time = match object.kind {
            HitObjectKind::Spinner { .. } => object.start_time,
            _ => object.start_time - preempt,
        };
        let time = TimeMs::new(time).map_err(|_| RoseaError::NumericalError {
            line: object.line,
            object_id: object.id,
        })?;
        if first.is_none_or(|previous: TimeMs| time.get() < previous.get()) {
            first = Some(time);
        }
    }
    Ok(Visibility {
        radius: 64.0_f32 * scale,
        preempt,
        fade_in: 400.0,
        first_visible_at: first.ok_or(RoseaError::EmptyBeatmap)?,
    })
}

pub(super) fn scale_and_preempt(cs: f32, ar: f32) -> (f32, f64) {
    let u = (f64::from(cs) - 5.0) / 5.0;
    let q = (1.0 - f64::from(0.7_f32) * u) as f32;
    let scale = (q / 2.0_f32) * 1.00041_f32;
    let ar = f64::from(ar);
    let preempt = if ar < 5.0 {
        (1200.0 + 120.0 * (5.0 - ar)).floor()
    } else {
        (1200.0 - 150.0 * (ar - 5.0)).floor()
    };
    (scale, preempt)
}
