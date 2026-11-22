//! Начисление очков и изменение статистики игровых событий.

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

/// Начисляет итог слайдера без изменения combo.
pub(super) fn apply_slider_result(
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
    *stats = updated;
    Ok(points)
}

/// Начисляет очки и combo за успешную голову слайдера.
pub fn apply_slider_head(stats: &mut Stats) -> Result<u64, RoseaError> {
    let mut updated = *stats;
    updated.score = updated
        .score
        .checked_add(30)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.current_combo = updated
        .current_combo
        .checked_add(1)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.max_combo = updated.max_combo.max(updated.current_combo);
    *stats = updated;
    Ok(30)
}

/// Применяет результат тика слайдера без итоговой оценки объекта.
pub fn apply_slider_tick(stats: &mut Stats, hit: bool) -> Result<u64, RoseaError> {
    if !hit {
        stats.current_combo = 0;
        return Ok(0);
    }
    let mut updated = *stats;
    updated.score = updated
        .score
        .checked_add(10)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.current_combo = updated
        .current_combo
        .checked_add(1)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.max_combo = updated.max_combo.max(updated.current_combo);
    *stats = updated;
    Ok(10)
}

/// Применяет результат повтора слайдера без итоговой оценки объекта.
pub fn apply_slider_repeat(stats: &mut Stats, hit: bool) -> Result<u64, RoseaError> {
    if !hit {
        stats.current_combo = 0;
        return Ok(0);
    }
    let mut updated = *stats;
    updated.score = updated
        .score
        .checked_add(30)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.current_combo = updated
        .current_combo
        .checked_add(1)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.max_combo = updated.max_combo.max(updated.current_combo);
    *stats = updated;
    Ok(30)
}

/// Применяет результат проверки хвоста без изменения combo при пропуске.
pub fn apply_slider_tail(stats: &mut Stats, hit: bool) -> Result<u64, RoseaError> {
    if !hit {
        return Ok(0);
    }
    let mut updated = *stats;
    updated.score = updated
        .score
        .checked_add(30)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.current_combo = updated
        .current_combo
        .checked_add(1)
        .ok_or(RoseaError::ScoreOverflow)?;
    updated.max_combo = updated.max_combo.max(updated.current_combo);
    *stats = updated;
    Ok(30)
}

#[cfg(test)]
mod slider_result_tests {
    use super::*;

    #[test]
    fn final_award_preserves_combo_and_checks_overflow() {
        let mut stats = Stats {
            score: 70,
            current_combo: 3,
            max_combo: 3,
            ..Stats::default()
        };
        assert_eq!(
            apply_slider_result(&mut stats, Judgement::Great, 5),
            Ok(420)
        );
        assert_eq!(
            (
                stats.score,
                stats.current_combo,
                stats.max_combo,
                stats.counts.count_300
            ),
            (490, 3, 3, 1)
        );
        let mut stats = Stats {
            score: 40,
            current_combo: 2,
            max_combo: 2,
            ..Stats::default()
        };
        assert_eq!(apply_slider_result(&mut stats, Judgement::Ok, 5), Ok(120));
        assert_eq!(
            (stats.score, stats.current_combo, stats.counts.count_100),
            (160, 2, 1)
        );
        stats.score = u64::MAX;
        let before = stats;
        assert_eq!(
            apply_slider_result(&mut stats, Judgement::Meh, 5),
            Err(RoseaError::ScoreOverflow)
        );
        assert_eq!(stats, before);
    }
}
