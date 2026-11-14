//! Оценка времени нажатия относительно начала объекта.

use crate::model::{Judgement, TimeMs};
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
