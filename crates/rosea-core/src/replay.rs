//! Нормализация времён и порядка записей реплея.

use crate::error::RoseaError;
use crate::model::{Parsed, Point, ReplayChange, ReplayInput, Warning};
use std::ops::Range;

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
    for record in input.records {
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
    let warnings = if changes.is_empty() {
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
}
