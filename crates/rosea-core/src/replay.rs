//! Нормализация времён и порядка записей реплея.

use crate::error::RoseaError;
use crate::model::{Parsed, Point, ReplayChange, ReplayInput, TimeMs, Warning};
use std::ops::Range;

/// Состояние двух игровых действий.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ButtonState {
    pub left: bool,
    pub right: bool,
}

impl ButtonState {
    pub fn from_keys(keys: u32) -> Self {
        Self {
            left: keys & 5 != 0,
            right: keys & 10 != 0,
        }
    }

    /// Новые нажатия относительно предыдущей записи.
    pub fn pressed_since(self, previous: Self) -> Self {
        Self {
            left: self.left && !previous.left,
            right: self.right && !previous.right,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct TimedRecord {
    pub index: u32,
    pub time_ms: i64,
    pub cursor: Point,
    pub keys: u32,
}

#[derive(Debug, PartialEq)]
pub struct ReplayTimeline {
    records: Vec<TimedRecord>,
    groups: Vec<Range<usize>>,
}

impl ReplayTimeline {
    pub fn record_count(&self) -> u32 {
        self.records.len() as u32
    }
    pub fn records(&self) -> &[TimedRecord] {
        &self.records
    }
    pub fn groups(&self) -> &[Range<usize>] {
        &self.groups
    }

    /// Позиция после обработки записей в данный момент.
    pub fn cursor_at(&self, time: TimeMs) -> Point {
        let time = time.get();
        let index = self.records.partition_point(|r| (r.time_ms as f64) <= time);
        if index == 0 {
            return Point { x: 256.0, y: 192.0 };
        }
        let left = &self.records[index - 1];
        if index == self.records.len() || left.time_ms as f64 == time {
            return left.cursor;
        }
        let right = &self.records[index];
        let progress = (time - left.time_ms as f64) / (right.time_ms - left.time_ms) as f64;
        let axis =
            |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * progress) as f32;
        Point {
            x: axis(left.cursor.x, right.cursor.x),
            y: axis(left.cursor.y, right.cursor.y),
        }
    }

    /// Входящая позиция перед обработкой группы записей в данный момент.
    pub fn cursor_before(&self, time: TimeMs) -> Point {
        let index = self
            .records
            .partition_point(|r| (r.time_ms as f64) < time.get());
        if index == 0 {
            return Point { x: 256.0, y: 192.0 };
        }
        if let Some(record) = self.records.get(index)
            && record.time_ms as f64 == time.get()
        {
            return record.cursor;
        }
        self.cursor_at(time)
    }
}

fn change_time(record: &mut TimedRecord, new_ms: i64, changes: &mut Vec<ReplayChange>) {
    if record.time_ms != new_ms {
        changes.push(ReplayChange::Time {
            index: record.index,
            old_ms: record.time_ms,
            new_ms,
        });
        record.time_ms = new_ms;
    }
}

pub fn normalize(input: ReplayInput) -> Result<Parsed<ReplayTimeline>, RoseaError> {
    let mut time = 0_i64;
    let mut records = Vec::with_capacity(input.records.len());
    let mut changes = Vec::new();
    let mut unknown_bits = 0_u32;
    let mut unknown_records = 0_u32;
    for record in input.records {
        let unknown = record.keys & !31;
        if unknown != 0 {
            unknown_bits |= unknown;
            unknown_records += 1;
        }
        time = time
            .checked_add(record.delta_ms)
            .ok_or(RoseaError::ReplayTimeOverflow {
                index: record.index,
            })?;
        records.push(TimedRecord {
            index: record.index,
            time_ms: time,
            cursor: record.cursor,
            keys: record.keys,
        });
    }
    if records.len() >= 2 && records[1].time_ms < records[0].time_ms {
        let first = records[0].time_ms;
        change_time(&mut records[1], first, &mut changes);
        change_time(&mut records[0], 0, &mut changes);
    }
    if records.len() >= 3 && records[0].time_ms > records[2].time_ms {
        let third = records[2].time_ms;
        change_time(&mut records[0], third, &mut changes);
        change_time(&mut records[1], third, &mut changes);
    }
    for index in [1, 0] {
        if records
            .get(index)
            .is_some_and(|record| record.cursor.x == 256.0 && record.cursor.y == -500.0)
        {
            let removed = records.remove(index);
            changes.push(ReplayChange::InitialDummy {
                index: removed.index,
            });
        }
    }
    let mut kept = Vec::with_capacity(records.len());
    for record in records {
        if kept
            .last()
            .is_some_and(|previous: &TimedRecord| record.time_ms < previous.time_ms)
        {
            changes.push(ReplayChange::BackwardFrame {
                index: record.index,
            });
        } else {
            if record.time_ms.unsigned_abs() > 86_400_000 {
                return Err(RoseaError::ResourceLimit {
                    resource: "replay time ms",
                    limit: 86_400_000,
                    observed: record.time_ms.unsigned_abs(),
                });
            }
            kept.push(record);
        }
    }
    if kept.is_empty() {
        return Err(RoseaError::NoReplayData { offset: 0 });
    }
    let mut groups: Vec<Range<usize>> = Vec::new();
    for (index, record) in kept.iter().enumerate() {
        if index > 0 && record.time_ms == kept[index - 1].time_ms {
            if let Some(group) = groups.last_mut() {
                group.end = index + 1;
            }
        } else {
            groups.push(index..index + 1);
        }
    }
    let mut warnings = if changes.is_empty() {
        Vec::new()
    } else {
        let changed_times = changes
            .iter()
            .filter(|change| matches!(change, ReplayChange::Time { .. }))
            .count() as u32;
        let initial_dummy_removed = changes
            .iter()
            .filter(|change| matches!(change, ReplayChange::InitialDummy { .. }))
            .count() as u32;
        let backward_removed = changes
            .iter()
            .filter(|change| matches!(change, ReplayChange::BackwardFrame { .. }))
            .count() as u32;
        vec![Warning::ReplayInputNormalized {
            changed_times,
            initial_dummy_removed,
            backward_removed,
            changes,
        }]
    };
    if unknown_bits != 0 {
        warnings.push(Warning::UnknownReplayKeys {
            bits: unknown_bits,
            records: unknown_records,
        });
    }
    Ok(Parsed {
        value: ReplayTimeline {
            records: kept,
            groups,
        },
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ReplayRecord;

    fn input(times: &[i64], dummy: &[usize]) -> ReplayInput {
        let mut prev = 0;
        let records = times
            .iter()
            .enumerate()
            .map(|(i, &time)| {
                let delta_ms = time - prev;
                prev = time;
                ReplayRecord {
                    index: i as u32,
                    delta_ms,
                    cursor: if dummy.contains(&i) {
                        Point {
                            x: 256.0,
                            y: -500.0,
                        }
                    } else {
                        Point { x: 1.0, y: 2.0 }
                    },
                    keys: 0,
                }
            })
            .collect();
        ReplayInput {
            records,
            seed: None,
        }
    }

    #[test]
    fn applies_prefix_rules_and_groups_equal_times() {
        let result = normalize(input(&[10, -10, 20, 20], &[])).unwrap();
        assert_eq!(
            result
                .value
                .records
                .iter()
                .map(|r| r.time_ms)
                .collect::<Vec<_>>(),
            [0, 10, 20, 20]
        );
        assert_eq!(result.value.groups(), &[0..1, 1..2, 2..4]);
        assert_eq!(
            result.warnings,
            vec![Warning::ReplayInputNormalized {
                changed_times: 2,
                initial_dummy_removed: 0,
                backward_removed: 0,
                changes: vec![
                    ReplayChange::Time {
                        index: 1,
                        old_ms: -10,
                        new_ms: 10
                    },
                    ReplayChange::Time {
                        index: 0,
                        old_ms: 10,
                        new_ms: 0
                    }
                ]
            }]
        );
        let result = normalize(input(&[50, 60, 40], &[])).unwrap();
        assert_eq!(
            result
                .value
                .records
                .iter()
                .map(|r| r.time_ms)
                .collect::<Vec<_>>(),
            [40, 40, 40]
        );
        assert_eq!(result.value.groups().len(), 1);
        assert_eq!(result.value.groups()[0], 0..3);
    }

    #[test]
    fn removes_only_initial_dummy_and_backwards_frames() {
        let result = normalize(input(&[0, 100, 110], &[0, 1])).unwrap();
        assert_eq!(result.value.record_count(), 1);
        assert_eq!(result.value.records()[0].time_ms, 110);
        assert_eq!(
            result.warnings,
            vec![Warning::ReplayInputNormalized {
                changed_times: 0,
                initial_dummy_removed: 2,
                backward_removed: 0,
                changes: vec![
                    ReplayChange::InitialDummy { index: 1 },
                    ReplayChange::InitialDummy { index: 0 }
                ]
            }]
        );
        let result = normalize(input(&[0, 20, 10, 30], &[2])).unwrap();
        assert_eq!(
            result
                .value
                .records()
                .iter()
                .map(|r| r.index)
                .collect::<Vec<_>>(),
            [0, 1, 3]
        );
        assert_eq!(result.value.records()[2].time_ms, 30);
        assert_eq!(
            result.warnings,
            vec![Warning::ReplayInputNormalized {
                changed_times: 0,
                initial_dummy_removed: 0,
                backward_removed: 1,
                changes: vec![ReplayChange::BackwardFrame { index: 2 }]
            }]
        );
        let result = normalize(input(&[0, 10, 10, 20], &[2])).unwrap();
        assert_eq!(result.value.record_count(), 4);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn rejects_overflow_empty_and_out_of_range() {
        assert!(matches!(
            normalize(input(&[], &[])),
            Err(RoseaError::NoReplayData { .. })
        ));
        assert!(matches!(
            normalize(input(&[0], &[0])),
            Err(RoseaError::NoReplayData { .. })
        ));
        assert!(matches!(
            normalize(input(&[86_400_001], &[])),
            Err(RoseaError::ResourceLimit { .. })
        ));
        assert!(normalize(input(&[-86_400_000], &[])).is_ok());
        let mut value = input(&[0, 0], &[]);
        value.records[0].delta_ms = i64::MAX;
        value.records[1].delta_ms = 1;
        assert!(matches!(
            normalize(value),
            Err(RoseaError::ReplayTimeOverflow { index: 1 })
        ));
    }

    #[test]
    fn interpolates_between_groups_and_keeps_boundary_positions() {
        let mut value = input(&[10, 10, 20, 20], &[]);
        for (record, cursor) in value.records.iter_mut().zip([
            Point { x: 0.0, y: 0.0 },
            Point { x: 10.0, y: 20.0 },
            Point { x: 30.0, y: 40.0 },
            Point { x: 90.0, y: 80.0 },
        ]) {
            record.cursor = cursor;
        }
        let timeline = normalize(value).unwrap().value;
        let at = |time| timeline.cursor_at(TimeMs::new(time).unwrap());
        let before = |time| timeline.cursor_before(TimeMs::new(time).unwrap());
        assert_eq!(at(9.0), Point { x: 256.0, y: 192.0 });
        assert_eq!(before(10.0), Point { x: 256.0, y: 192.0 });
        assert_eq!(at(10.0), Point { x: 10.0, y: 20.0 });
        assert_eq!(at(15.0), Point { x: 20.0, y: 30.0 });
        assert_eq!(at(19.5), Point { x: 29.0, y: 39.0 });
        assert_eq!(before(20.0), Point { x: 30.0, y: 40.0 });
        assert_eq!(at(20.0), Point { x: 90.0, y: 80.0 });
        assert_eq!(at(100.0), Point { x: 90.0, y: 80.0 });
    }

    #[test]
    fn interpolation_uses_f64_for_large_coordinates_and_single_record() {
        let mut value = input(&[0, 10], &[]);
        value.records[0].cursor.x = -f32::MAX;
        value.records[1].cursor.x = f32::MAX;
        let timeline = normalize(value).unwrap().value;
        assert_eq!(timeline.cursor_at(TimeMs::new(5.0).unwrap()).x, 0.0);

        let timeline = normalize(input(&[10], &[])).unwrap().value;
        assert_eq!(
            timeline.cursor_at(TimeMs::new(9.0).unwrap()),
            Point { x: 256.0, y: 192.0 }
        );
        assert_eq!(
            timeline.cursor_at(TimeMs::new(10.0).unwrap()),
            Point { x: 1.0, y: 2.0 }
        );
        assert_eq!(
            timeline.cursor_at(TimeMs::new(11.0).unwrap()),
            Point { x: 1.0, y: 2.0 }
        );
    }

    #[test]
    fn maps_buttons_and_detects_only_new_presses() {
        let none = ButtonState::default();
        let left = ButtonState::from_keys(5);
        let right = ButtonState::from_keys(10);
        assert_eq!(
            left,
            ButtonState {
                left: true,
                right: false
            }
        );
        assert_eq!(
            right,
            ButtonState {
                left: false,
                right: true
            }
        );
        assert_eq!(left.pressed_since(none), left);
        assert_eq!(left.pressed_since(ButtonState::from_keys(1)), none);
        assert_eq!(ButtonState::from_keys(4).pressed_since(left), none);
        assert_eq!(ButtonState::from_keys(15).pressed_since(left), right);
        assert_eq!(ButtonState::from_keys(16 | 0x8000_0000), none);
        assert_eq!(none.pressed_since(left), none);
    }

    #[test]
    fn reports_unknown_key_bits_once_and_preserves_the_masks() {
        let mut value = input(&[0, 10, 20], &[]);
        value.records[0].keys = 1 | 32;
        value.records[1].keys = 16;
        value.records[2].keys = 4 | 64 | 32;
        let result = normalize(value).unwrap();
        assert_eq!(
            result
                .value
                .records()
                .iter()
                .map(|r| r.keys)
                .collect::<Vec<_>>(),
            [33, 16, 100]
        );
        assert_eq!(
            result.warnings,
            vec![Warning::UnknownReplayKeys {
                bits: 96,
                records: 2
            }]
        );

        let mut value = input(&[0, 20, 10], &[]);
        value.records[2].keys = 32;
        let result = normalize(value).unwrap();
        assert!(matches!(
            result.warnings.as_slice(),
            [
                Warning::ReplayInputNormalized { .. },
                Warning::UnknownReplayKeys {
                    bits: 32,
                    records: 1
                }
            ]
        ));
    }
}
