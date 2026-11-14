//! Параметры сложности для подсчёта очков.

use crate::error::RoseaError;
use crate::model::Beatmap;

const MAX_TIME: f64 = 86_400_000.0;

pub(super) fn score_parameters(map: &Beatmap) -> Result<(i64, u32), RoseaError> {
    let first = map.objects().first().ok_or(RoseaError::EmptyBeatmap)?;
    let last = map.objects().last().ok_or(RoseaError::EmptyBeatmap)?;
    let mut break_ms = 0_i64;
    for period in map.breaks() {
        for time in [period.start_time, period.end_time] {
            if time.abs() > MAX_TIME {
                return Err(RoseaError::ResourceLimit {
                    resource: "absolute_time_ms",
                    limit: MAX_TIME as u64,
                    observed: time.abs().ceil() as u64,
                });
            }
        }
        break_ms +=
            period.end_time.round_ties_even() as i64 - period.start_time.round_ties_even() as i64;
    }
    let raw_ms = last.start_time.round_ties_even() as i64
        - first.start_time.round_ties_even() as i64
        - break_ms;
    let drain_seconds = raw_ms / 1000;
    let ratio = if drain_seconds == 0 {
        16.0
    } else {
        (f64::from(map.object_count()) / drain_seconds as f64 * 8.0).clamp(0.0, 16.0)
    };
    let difficulty = map.difficulty();
    let x = (((f64::from(difficulty.hp) + f64::from(difficulty.od)) + f64::from(difficulty.cs))
        + ratio)
        / 38.0
        * 5.0;
    Ok((drain_seconds, x.round_ties_even() as u32))
}
