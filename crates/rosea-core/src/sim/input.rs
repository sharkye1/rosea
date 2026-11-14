//! Последовательное применение записей реплея и завершение ввода.

use crate::model::Point;
use crate::replay::ButtonState;

use super::schedule::TimeGroup;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputAction {
    Left,
    Right,
}

/// Изменения ввода в порядке обработки записей одной группы.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum InputEvent {
    Record {
        index: u32,
        cursor: Point,
        buttons: ButtonState,
    },
    Press {
        index: u32,
        cursor: Point,
        action: InputAction,
    },
    RecordEnd {
        index: u32,
        cursor: Point,
        buttons: ButtonState,
    },
    End {
        cursor: Point,
    },
}

/// Текущая позиция курсора и состояние действий.
#[derive(Debug, PartialEq)]
pub struct InputState {
    cursor: Point,
    buttons: ButtonState,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            cursor: Point { x: 256.0, y: 192.0 },
            buttons: ButtonState::default(),
        }
    }
}

impl InputState {
    pub fn cursor(&self) -> Point {
        self.cursor
    }

    pub fn buttons(&self) -> ButtonState {
        self.buttons
    }

    /// Применяет записи группы, а после последней группы отпускает действия.
    pub fn apply_group(&mut self, group: &TimeGroup<'_>, mut emit: impl FnMut(InputEvent)) {
        for record in group.records {
            let buttons = ButtonState::from_keys(record.keys);
            let pressed = buttons.pressed_since(self.buttons);
            self.cursor = record.cursor;
            self.buttons = buttons;
            emit(InputEvent::Record {
                index: record.index,
                cursor: self.cursor,
                buttons,
            });
            if pressed.left {
                emit(InputEvent::Press {
                    index: record.index,
                    cursor: self.cursor,
                    action: InputAction::Left,
                });
            }
            if pressed.right {
                emit(InputEvent::Press {
                    index: record.index,
                    cursor: self.cursor,
                    action: InputAction::Right,
                });
            }
            emit(InputEvent::RecordEnd {
                index: record.index,
                cursor: self.cursor,
                buttons,
            });
        }
        if group.last_input {
            self.buttons = ButtonState::default();
            emit(InputEvent::End {
                cursor: self.cursor,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ReplayInput, ReplayRecord, TimeMs};
    use crate::parse::osu;
    use crate::prepare::prepare_map;
    use crate::replay::normalize;
    use crate::sim::schedule::Schedule;

    #[test]
    fn applies_records_and_releases_after_last_group() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,-110,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let input = ReplayInput {
            records: [(10, 15), (0, 15), (0, 0), (10, 4)]
                .into_iter()
                .enumerate()
                .map(|(index, (delta_ms, keys))| ReplayRecord {
                    index: index as u32,
                    delta_ms,
                    cursor: Point {
                        x: index as f32,
                        y: 0.0,
                    },
                    keys,
                })
                .collect(),
            seed: None,
        };
        let replay = normalize(input).unwrap().value;
        let mut schedule = Schedule::new(&map, &replay);
        let mut state = InputState::default();
        let mut events = Vec::new();
        schedule
            .advance_to(TimeMs::new(10.0).unwrap(), |group| {
                state.apply_group(&group, |event| events.push(event));
            })
            .unwrap();
        assert_eq!(state.cursor(), Point { x: 2.0, y: 0.0 });
        assert_eq!(state.buttons(), ButtonState::default());
        assert_eq!(
            events
                .iter()
                .filter_map(|event| match event {
                    InputEvent::Record { index, cursor, .. } => Some((*index, cursor.x)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(0, 0.0), (1, 1.0), (2, 2.0)]
        );
        assert_eq!(
            events
                .iter()
                .filter_map(|event| match event {
                    InputEvent::Press {
                        index,
                        cursor,
                        action,
                    } => Some((*index, *cursor, *action)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [
                (0, Point { x: 0.0, y: 0.0 }, InputAction::Left),
                (0, Point { x: 0.0, y: 0.0 }, InputAction::Right)
            ]
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, InputEvent::RecordEnd { .. }))
                .count(),
            3
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, InputEvent::End { .. }))
        );
        events.clear();
        schedule
            .advance_to(TimeMs::new(20.0).unwrap(), |group| {
                state.apply_group(&group, |event| events.push(event));
            })
            .unwrap();
        assert_eq!(state.cursor(), Point { x: 3.0, y: 0.0 });
        assert_eq!(state.buttons(), ButtonState::default());
        assert!(matches!(
            events.as_slice(),
            [
                InputEvent::Record { index: 3, .. },
                InputEvent::Press {
                    index: 3,
                    action: InputAction::Left,
                    ..
                },
                InputEvent::RecordEnd { index: 3, .. },
                InputEvent::End { .. }
            ]
        ));
        events.clear();
        schedule
            .advance_to(TimeMs::new(20.0).unwrap(), |group| {
                state.apply_group(&group, |event| events.push(event));
            })
            .unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn single_held_record_is_processed_before_release() {
        let record = crate::replay::TimedRecord {
            index: 7,
            time_ms: 100,
            cursor: Point { x: 50.0, y: 60.0 },
            keys: 5,
        };
        let group = TimeGroup {
            time: TimeMs::new(100.0).unwrap(),
            records: std::slice::from_ref(&record),
            map_events: &[],
            last_input: true,
        };
        let mut state = InputState::default();
        let mut events = Vec::new();
        state.apply_group(&group, |event| events.push(event));
        assert!(matches!(
            events.as_slice(),
            [
                InputEvent::Record { index: 7, .. },
                InputEvent::Press {
                    index: 7,
                    action: InputAction::Left,
                    ..
                },
                InputEvent::RecordEnd { index: 7, .. },
                InputEvent::End { .. }
            ]
        ));
        assert_eq!(state.buttons(), ButtonState::default());
        assert_eq!(state.cursor(), record.cursor);
    }
}
