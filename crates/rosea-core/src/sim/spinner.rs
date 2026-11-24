//! Накопление угла по интервалам временной шкалы.

use std::f64::consts::PI;
use std::ops::Range;

use crate::error::RoseaError;
use crate::model::{Judgement, ObjectId, Point, TimeMs};
use crate::prepare::ObjectTimes;
use crate::replay::{ButtonState, ReplayTimeline};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum HalfTurnAward {
    Points,
    Bonus,
}

pub(super) fn half_turn_award(half: u32, req_bonus: u32) -> Option<HalfTurnAward> {
    if half > req_bonus && (half - req_bonus).is_multiple_of(2) {
        Some(HalfTurnAward::Bonus)
    } else if half > 1 && half.is_multiple_of(2) {
        Some(HalfTurnAward::Points)
    } else {
        None
    }
}

pub(super) struct SpinnerState {
    object_id: ObjectId,
    line: usize,
    start: TimeMs,
    end: TimeMs,
    angle: f64,
    half_turns: u32,
}

impl SpinnerState {
    pub(super) fn new(times: &ObjectTimes, line: usize) -> Self {
        Self {
            object_id: times.object_id,
            line,
            start: times.start,
            end: times.end,
            angle: 0.0,
            half_turns: 0,
        }
    }

    pub(super) fn advance(
        &mut self,
        previous: TimeMs,
        time: TimeMs,
        buttons: ButtonState,
        replay: &ReplayTimeline,
    ) -> Result<Range<u32>, RoseaError> {
        let left = if previous.get() < self.start.get() {
            self.start
        } else {
            previous
        };
        let right = if time.get() > self.end.get() {
            self.end
        } else {
            time
        };
        if left.get() >= right.get() || !(buttons.left || buttons.right) {
            return Ok(self.half_turns..self.half_turns);
        }
        let angle = self.angle + angle_between(replay.cursor_at(left), replay.cursor_before(right));
        let half_turns = completed_halves(angle).ok_or(RoseaError::NumericalError {
            line: self.line,
            object_id: self.object_id,
        })?;
        // Индексы с нуля: номер полуоборота на единицу больше индекса.
        let crossed = self.half_turns..half_turns;
        self.angle = angle;
        self.half_turns = half_turns;
        Ok(crossed)
    }

    pub(super) fn judgement(&self, req_half: u32) -> Judgement {
        if req_half == 0 || self.half_turns > req_half {
            Judgement::Great
        } else if self.half_turns >= (req_half - 1).max(1) {
            Judgement::Ok
        } else if self.half_turns >= req_half / 4 {
            Judgement::Meh
        } else {
            Judgement::Miss
        }
    }
}

fn completed_halves(angle: f64) -> Option<u32> {
    let halves = (angle / PI).floor();
    if !(0.0..=f64::from(u32::MAX)).contains(&halves) {
        return None;
    }
    Some(halves as u32)
}

fn angle_between(left: Point, right: Point) -> f64 {
    let ux = f64::from(left.x) - 256.0;
    let uy = f64::from(left.y) - 192.0;
    let vx = f64::from(right.x) - 256.0;
    let vy = f64::from(right.y) - 192.0;
    if (ux == 0.0 && uy == 0.0) || (vx == 0.0 && vy == 0.0) {
        return 0.0;
    }
    (ux * vy - uy * vx).atan2(ux * vx + uy * vy).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ReplayInput, ReplayRecord};
    use crate::replay::normalize;

    fn time(ms: f64) -> TimeMs {
        TimeMs::new(ms).unwrap()
    }

    fn state(start: f64, end: f64) -> SpinnerState {
        SpinnerState::new(
            &ObjectTimes {
                object_id: ObjectId(0),
                start: time(start),
                end: time(end),
                head_deadline: None,
                latest_resolution: time(end),
            },
            1,
        )
    }

    fn replay(records: &[(i64, f32, f32, u32)]) -> ReplayTimeline {
        normalize(ReplayInput {
            records: records
                .iter()
                .enumerate()
                .map(|(index, &(delta_ms, x, y, keys))| ReplayRecord {
                    index: index as u32,
                    delta_ms,
                    cursor: Point { x, y },
                    keys,
                })
                .collect(),
            seed: None,
        })
        .unwrap()
        .value
    }

    #[test]
    fn clips_to_spinner_and_uses_incoming_cursor_with_old_buttons() {
        let replay = replay(&[
            (100, 356.0, 192.0, 1),
            (100, 256.0, 292.0, 0),
            (0, 156.0, 192.0, 0),
        ]);
        let mut spinner = state(150.0, 200.0);
        spinner
            .advance(
                time(100.0),
                time(200.0),
                replay.buttons_at(time(100.0)),
                &replay,
            )
            .unwrap();
        assert_eq!(spinner.angle, std::f64::consts::FRAC_PI_4);

        let mut spinner = state(100.0, 150.0);
        spinner
            .advance(
                time(100.0),
                time(200.0),
                replay.buttons_at(time(100.0)),
                &replay,
            )
            .unwrap();
        assert_eq!(spinner.angle, std::f64::consts::FRAC_PI_4);
    }

    #[test]
    fn zero_vectors_and_intervals_add_no_angle() {
        let replay = replay(&[
            (100, 156.0, 192.0, 1),
            (100, 256.0, 192.0, 1),
            (100, 156.0, 192.0, 1),
            (0, 356.0, 192.0, 1),
        ]);
        let mut spinner = state(0.0, 500.0);
        for (left, right) in [(0.0, 100.0), (100.0, 200.0), (200.0, 300.0), (300.0, 300.0)] {
            spinner
                .advance(
                    time(left),
                    time(right),
                    replay.buttons_at(time(left)),
                    &replay,
                )
                .unwrap();
        }
        assert_eq!(spinner.angle, 0.0);
    }

    #[test]
    fn reversing_direction_adds_angle_only_while_held() {
        let replay = replay(&[
            (100, 356.0, 192.0, 1),
            (100, 256.0, 292.0, 2),
            (100, 356.0, 192.0, 0),
            (100, 156.0, 192.0, 1),
        ]);
        let mut spinner = state(0.0, 500.0);
        for (left, right, expected) in [
            (100.0, 200.0, 0..0),
            (200.0, 300.0, 0..1),
            (300.0, 400.0, 1..1),
        ] {
            assert_eq!(
                spinner
                    .advance(
                        time(left),
                        time(right),
                        replay.buttons_at(time(left)),
                        &replay
                    )
                    .unwrap(),
                expected
            );
        }
        assert_eq!(spinner.angle, std::f64::consts::PI);
    }

    #[test]
    fn half_turn_boundaries_are_exact_and_checked() {
        assert_eq!(completed_halves(PI.next_down()), Some(0));
        assert_eq!(completed_halves(PI), Some(1));
        assert_eq!(completed_halves(2.0 * PI), Some(2));
        for angle in [f64::NAN, f64::INFINITY, -PI, 2.0 * f64::from(u32::MAX) * PI] {
            assert_eq!(completed_halves(angle), None);
        }
    }

    #[test]
    fn bonus_threshold_preserves_parity_and_takes_precedence() {
        use HalfTurnAward::{Bonus, Points};

        assert_eq!(half_turn_award(1, 16), None);
        assert_eq!(half_turn_award(16, 16), Some(Points));
        assert_eq!(half_turn_award(17, 16), None);
        assert_eq!(half_turn_award(18, 16), Some(Bonus));
        assert_eq!(half_turn_award(15, 15), None);
        assert_eq!(half_turn_award(16, 15), Some(Points));
        assert_eq!(half_turn_award(17, 15), Some(Bonus));
        assert_eq!(half_turn_award(18, 15), Some(Points));
    }

    #[test]
    fn spinner_result_uses_inclusive_thresholds() {
        let mut spinner = state(0.0, 2000.0);
        assert_eq!(spinner.judgement(0), Judgement::Great);
        assert_eq!(spinner.judgement(1), Judgement::Meh);
        for (halves, judgement) in [
            (0, Judgement::Miss),
            (2, Judgement::Miss),
            (3, Judgement::Meh),
            (11, Judgement::Meh),
            (12, Judgement::Ok),
            (13, Judgement::Ok),
            (14, Judgement::Great),
        ] {
            spinner.half_turns = halves;
            assert_eq!(spinner.judgement(13), judgement);
        }
    }
}
