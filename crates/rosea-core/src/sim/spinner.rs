//! Накопление угла по интервалам временной шкалы.

use crate::model::{Point, TimeMs};
use crate::prepare::ObjectTimes;
use crate::replay::{ButtonState, ReplayTimeline};

pub(super) struct SpinnerState {
    start: TimeMs,
    end: TimeMs,
    angle: f64,
}

impl SpinnerState {
    pub(super) fn new(times: &ObjectTimes) -> Self {
        Self {
            start: times.start,
            end: times.end,
            angle: 0.0,
        }
    }

    pub(super) fn advance(
        &mut self,
        previous: TimeMs,
        time: TimeMs,
        buttons: ButtonState,
        replay: &ReplayTimeline,
    ) {
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
            return;
        }
        self.angle += angle_between(replay.cursor_at(left), replay.cursor_before(right));
    }
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
    use crate::model::{ObjectId, ReplayInput, ReplayRecord};
    use crate::replay::normalize;

    fn time(ms: f64) -> TimeMs {
        TimeMs::new(ms).unwrap()
    }

    fn state(start: f64, end: f64) -> SpinnerState {
        SpinnerState::new(&ObjectTimes {
            object_id: ObjectId(0),
            start: time(start),
            end: time(end),
            head_deadline: None,
            latest_resolution: time(end),
        })
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
        spinner.advance(
            time(100.0),
            time(200.0),
            replay.buttons_at(time(100.0)),
            &replay,
        );
        assert_eq!(spinner.angle, std::f64::consts::FRAC_PI_4);

        let mut spinner = state(100.0, 150.0);
        spinner.advance(
            time(100.0),
            time(200.0),
            replay.buttons_at(time(100.0)),
            &replay,
        );
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
            spinner.advance(
                time(left),
                time(right),
                replay.buttons_at(time(left)),
                &replay,
            );
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
        for (left, right) in [(100.0, 200.0), (200.0, 300.0), (300.0, 400.0)] {
            spinner.advance(
                time(left),
                time(right),
                replay.buttons_at(time(left)),
                &replay,
            );
        }
        assert_eq!(spinner.angle, std::f64::consts::PI);
    }
}
