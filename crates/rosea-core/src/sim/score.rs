//! Начисление очков и статистика для результатов кругов.

use crate::error::RoseaError;
use crate::model::{Judgement, Stats};

/// Начисление за итоговую оценку с использованием combo до события.
pub fn award(judgement: Judgement, combo: u32, difficulty: u32) -> Result<u64, RoseaError> {
    let value: u64 = match judgement {
        Judgement::Great => 300,
        Judgement::Ok => 100,
        Judgement::Meh => 50,
        Judgement::Miss => return Ok(0),
    };
    let bonus = (value / 25)
        .checked_mul(u64::from(combo.saturating_sub(1)))
        .and_then(|n| n.checked_mul(u64::from(difficulty)))
        .ok_or(RoseaError::ScoreOverflow)?;
    value.checked_add(bonus).ok_or(RoseaError::ScoreOverflow)
}

/// Применяет результат круга и возвращает начисленные очки.
pub fn apply_circle(
    stats: &mut Stats,
    judgement: Judgement,
    difficulty: u32,
) -> Result<u64, RoseaError> {
    let points = award(judgement, stats.current_combo, difficulty)?;
    let mut updated = *stats;
    updated.score = updated
        .score
        .checked_add(points)
        .ok_or(RoseaError::ScoreOverflow)?;
    let count = match judgement {
        Judgement::Great => &mut updated.counts.count_300,
        Judgement::Ok => &mut updated.counts.count_100,
        Judgement::Meh => &mut updated.counts.count_50,
        Judgement::Miss => &mut updated.counts.count_miss,
    };
    *count = count.checked_add(1).ok_or(RoseaError::ScoreOverflow)?;
    updated.current_combo = if judgement == Judgement::Miss {
        0
    } else {
        updated
            .current_combo
            .checked_add(1)
            .ok_or(RoseaError::ScoreOverflow)?
    };
    updated.max_combo = updated.max_combo.max(updated.current_combo);
    *stats = updated;
    Ok(points)
}
