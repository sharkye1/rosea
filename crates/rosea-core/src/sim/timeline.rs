//! Применение ввода и выдача событий карты в порядке временных групп.

use crate::error::RoseaError;
use crate::model::TimeMs;
use crate::prepare::{MapEvent, MapEventKind, PreparedMap};
use crate::replay::{ButtonState, ReplayTimeline};

use super::input::{InputEvent, InputState};
use super::schedule::Schedule;

/// Событие ввода или подготовленное событие карты.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TimelineEvent {
    Input(InputEvent),
    Map(MapEvent),
}

pub(super) enum TimelineStep<'a> {
    Begin {
        previous: Option<TimeMs>,
        buttons: ButtonState,
    },
    Input(InputEvent),
    Map(&'a [MapEvent]),
}

/// Продвигает временные группы и состояние ввода.
pub struct Timeline<'a> {
    schedule: Schedule<'a>,
    input: InputState,
    previous: Option<TimeMs>,
}

impl<'a> Timeline<'a> {
    pub fn new(map: &'a PreparedMap, replay: &'a ReplayTimeline) -> Self {
        Self {
            schedule: Schedule::new(map, replay),
            input: InputState::default(),
            previous: None,
        }
    }

    pub fn input(&self) -> &InputState {
        &self.input
    }

    /// Выдаёт события полных групп до указанного времени включительно.
    pub fn advance_to(
        &mut self,
        time: TimeMs,
        mut emit: impl FnMut(TimeMs, TimelineEvent),
    ) -> Result<(), RoseaError> {
        self.advance_grouped(time, |time, step| match step {
            TimelineStep::Begin { .. } => {}
            TimelineStep::Input(event) => emit(time, TimelineEvent::Input(event)),
            TimelineStep::Map(events) => {
                for &event in events {
                    emit(time, TimelineEvent::Map(event));
                }
            }
        })
    }

    /// Передаёт события карты срезами до и после фазы ввода.
    pub(super) fn advance_grouped(
        &mut self,
        time: TimeMs,
        mut emit: impl FnMut(TimeMs, TimelineStep<'a>),
    ) -> Result<(), RoseaError> {
        let input = &mut self.input;
        let previous = &mut self.previous;
        self.schedule.advance_to(time, |group| {
            emit(
                group.time,
                TimelineStep::Begin {
                    previous: previous.replace(group.time),
                    buttons: input.buttons(),
                },
            );
            // В подготовленной группе начала спиннеров стоят перед остальными событиями.
            let spinner_count = group
                .map_events
                .partition_point(|event| event.kind == MapEventKind::SpinnerStart);
            let (spinner_starts, map_events) = group.map_events.split_at(spinner_count);
            if !spinner_starts.is_empty() {
                emit(group.time, TimelineStep::Map(spinner_starts));
            }
            input.apply_group(&group, |event| emit(group.time, TimelineStep::Input(event)));
            emit(group.time, TimelineStep::Map(map_events));
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, ReplayInput, ReplayRecord};
    use crate::parse::osu;
    use crate::prepare::prepare_map;
    use crate::replay::{ButtonState, normalize};
    use crate::sim::input::{InputAction, InputEvent};

    #[test]
    fn processes_input_before_parts_and_deadlines_at_the_same_time() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|100:0,1,0\n100,100,880,1,0\n256,192,1000,8,0,1000";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let input = ReplayInput {
            records: vec![
                ReplayRecord {
                    index: 0,
                    delta_ms: 1000,
                    cursor: Point { x: 10.0, y: 20.0 },
                    keys: 15,
                },
                ReplayRecord {
                    index: 1,
                    delta_ms: 0,
                    cursor: Point { x: 30.0, y: 40.0 },
                    keys: 15,
                },
            ],
            seed: None,
        };
        let replay = normalize(input).unwrap().value;
        let mut timeline = Timeline::new(&map, &replay);
        let mut events = Vec::new();
        timeline
            .advance_to(TimeMs::new(1000.0).unwrap(), |time, event| {
                events.push((time.get(), event))
            })
            .unwrap();
        let names: Vec<_> = events
            .iter()
            .map(|(_, event)| match event {
                TimelineEvent::Map(event) => match event.kind {
                    MapEventKind::SpinnerStart => "spinner_start",
                    MapEventKind::SliderStart => "slider_start",
                    MapEventKind::SliderTailCheck => "tail",
                    MapEventKind::SliderEnd => "slider_end",
                    MapEventKind::CircleDeadline => "circle_deadline",
                    MapEventKind::SpinnerEnd => "spinner_end",
                    _ => "other",
                },
                TimelineEvent::Input(InputEvent::Record { index: 0, .. }) => "record_0",
                TimelineEvent::Input(InputEvent::Record { index: 1, .. }) => "record_1",
                TimelineEvent::Input(InputEvent::Press {
                    action: InputAction::Left,
                    ..
                }) => "left",
                TimelineEvent::Input(InputEvent::Press {
                    action: InputAction::Right,
                    ..
                }) => "right",
                TimelineEvent::Input(InputEvent::RecordEnd { index: 0, .. }) => "record_end_0",
                TimelineEvent::Input(InputEvent::RecordEnd { index: 1, .. }) => "record_end_1",
                TimelineEvent::Input(InputEvent::End { .. }) => "eof",
                _ => "other",
            })
            .collect();
        assert_eq!(
            names,
            [
                "spinner_start",
                "record_0",
                "left",
                "right",
                "record_end_0",
                "record_1",
                "record_end_1",
                "eof",
                "slider_start",
                "tail",
                "slider_end",
                "circle_deadline",
                "spinner_end"
            ]
        );
        assert!(events.iter().all(|(time, _)| *time == 1000.0));
        assert_eq!(timeline.input().cursor(), Point { x: 30.0, y: 40.0 });
        assert_eq!(timeline.input().buttons(), ButtonState::default());
        events.clear();
        timeline
            .advance_to(TimeMs::new(1000.0).unwrap(), |time, event| {
                events.push((time.get(), event))
            })
            .unwrap();
        assert!(events.is_empty());
        assert_eq!(
            timeline.advance_to(TimeMs::new(999.0).unwrap(), |_, _| {}),
            Err(RoseaError::TimeWentBackwards)
        );
    }
}
