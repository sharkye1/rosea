//! Прогон кругов по объединённой временной шкале.

use crate::error::RoseaError;
use crate::model::{Judgement, ObjectId, Point, Stats, TimeMs};
use crate::prepare::{MapEventKind, PreparedMap, StackedObjectKind};
use crate::replay::{ButtonState, ReplayTimeline};

use super::hit::{inside_radius, judge_click};
use super::input::InputEvent;
use super::score::apply_circle;
use super::slider::SliderState;
use super::timeline::{Timeline, TimelineEvent};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    Circle,
    SliderTick,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SimEvent {
    pub time: TimeMs,
    pub object_id: ObjectId,
    pub kind: EventKind,
    pub part_index: Option<u32>,
    pub part_hit: Option<bool>,
    pub judgement: Option<Judgement>,
    pub score_delta: u64,
    pub combo_before: u32,
    pub combo_after: u32,
}

/// Частичный прогон кругов и тиков слайдера.
pub struct Simulator<'a> {
    map: &'a PreparedMap,
    timeline: Timeline<'a>,
    resolved: Vec<bool>,
    sliders: Vec<Option<SliderState<'a>>>,
    cursor: Point,
    buttons: ButtonState,
    stats: Stats,
    failed: bool,
    end_time: TimeMs,
    current_time: Option<TimeMs>,
}

struct EventProcessor<'map, 'state, 'sim> {
    map: &'map PreparedMap,
    resolved: &'state mut [bool],
    sliders: &'state mut [Option<SliderState<'sim>>],
    cursor: &'state mut Point,
    buttons: &'state mut ButtonState,
    stats: &'state mut Stats,
    emit: &'state mut dyn FnMut(&SimEvent),
}

impl<'a> Simulator<'a> {
    pub fn new(map: &'a PreparedMap, input: &'a ReplayTimeline) -> Result<Self, RoseaError> {
        if let Some(object) = map.objects().iter().find(|object| {
            !matches!(
                object.kind(),
                StackedObjectKind::Circle | StackedObjectKind::Slider(_)
            )
        }) {
            return Err(RoseaError::UnsupportedSimulationObject {
                object_id: object.object_id(),
            });
        }
        let last_input = input
            .records()
            .last()
            .ok_or(RoseaError::NoReplayData { offset: 0 })?;
        let mut end_time = TimeMs::new(last_input.time_ms as f64)?;
        for object in map.object_times() {
            if object.latest_resolution.get() > end_time.get() {
                end_time = object.latest_resolution;
            }
        }
        Ok(Self {
            map,
            timeline: Timeline::new(map, input),
            resolved: vec![false; map.objects().len()],
            sliders: map.objects().iter().map(SliderState::new).collect(),
            cursor: Point { x: 256.0, y: 192.0 },
            buttons: ButtonState::default(),
            stats: Stats::default(),
            failed: false,
            end_time,
            current_time: None,
        })
    }

    pub fn end_time(&self) -> TimeMs {
        self.end_time
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn advance_to(&mut self, time: TimeMs) -> Result<(), RoseaError> {
        self.advance_with(time, &mut |_| {})
    }

    pub fn advance_with(
        &mut self,
        time: TimeMs,
        emit: &mut dyn FnMut(&SimEvent),
    ) -> Result<(), RoseaError> {
        if self.failed {
            return Err(RoseaError::ScoreOverflow);
        }
        let map = self.map;
        let timeline = &mut self.timeline;
        let resolved = &mut self.resolved;
        let sliders = &mut self.sliders;
        let cursor = &mut self.cursor;
        let buttons = &mut self.buttons;
        let stats = &mut self.stats;
        let mut processor = EventProcessor {
            map,
            resolved,
            sliders,
            cursor,
            buttons,
            stats,
            emit,
        };
        let mut failure = None;
        timeline.advance_to(time, |time, event| {
            if failure.is_none()
                && let Err(error) = processor.process(time, event)
            {
                failure = Some(error);
            }
        })?;
        if let Some(error) = failure {
            self.failed = true;
            return Err(error);
        }
        self.current_time = Some(time);
        Ok(())
    }

    pub fn finish(self) -> Result<Stats, RoseaError> {
        self.finish_with(&mut |_| {})
    }

    pub fn finish_with(mut self, emit: &mut dyn FnMut(&SimEvent)) -> Result<Stats, RoseaError> {
        let time = self
            .current_time
            .filter(|time| time.get() > self.end_time.get())
            .unwrap_or(self.end_time);
        self.advance_with(time, emit)?;
        let counts = self.stats.counts;
        let actual = u64::from(counts.count_300)
            + u64::from(counts.count_100)
            + u64::from(counts.count_50)
            + u64::from(counts.count_miss);
        let expected = self.map.objects().len() as u32;
        if actual != u64::from(expected) || self.resolved.iter().any(|resolved| !resolved) {
            return Err(RoseaError::IncompleteSimulation { expected, actual });
        }
        Ok(self.stats)
    }
}

impl EventProcessor<'_, '_, '_> {
    fn process(&mut self, time: TimeMs, event: TimelineEvent) -> Result<(), RoseaError> {
        match event {
            TimelineEvent::Input(InputEvent::Record {
                cursor: record_cursor,
                buttons: record_buttons,
                ..
            }) => {
                *self.cursor = record_cursor;
                *self.buttons = record_buttons;
            }
            TimelineEvent::Input(
                input @ InputEvent::RecordEnd {
                    cursor: record_cursor,
                    buttons: record_buttons,
                    ..
                },
            ) => {
                *self.cursor = record_cursor;
                *self.buttons = record_buttons;
                for slider in self.sliders.iter_mut().flatten() {
                    slider.observe_input_event(time, input, self.map.visibility().radius)?;
                }
            }
            TimelineEvent::Input(input @ InputEvent::End { cursor: end_cursor }) => {
                *self.cursor = end_cursor;
                *self.buttons = ButtonState::default();
                for slider in self.sliders.iter_mut().flatten() {
                    slider.observe_input_event(time, input, self.map.visibility().radius)?;
                }
            }
            TimelineEvent::Input(InputEvent::Press {
                cursor: click_cursor,
                ..
            }) => {
                *self.cursor = click_cursor;
                let candidate = self
                    .map
                    .objects()
                    .iter()
                    .filter_map(|object| {
                        if object.kind() != StackedObjectKind::Circle
                            || self.resolved[object.object_id().0 as usize]
                            || !inside_radius(
                                click_cursor,
                                object.position(),
                                self.map.visibility().radius,
                            )
                        {
                            return None;
                        }
                        let start = TimeMs::new(object.start_time()).ok()?;
                        let judgement =
                            judge_click(self.map.hit_windows(), start, time.get() as i64)?;
                        Some((object, judgement))
                    })
                    .min_by(|(a, _), (b, _)| {
                        a.start_time()
                            .total_cmp(&b.start_time())
                            .then(a.object_id().0.cmp(&b.object_id().0))
                    });
                if let Some((object, judgement)) = candidate {
                    let id = object.object_id();
                    let combo_before = self.stats.current_combo;
                    let score_delta =
                        apply_circle(self.stats, judgement, self.map.difficulty_multiplier())?;
                    self.resolved[id.0 as usize] = true;
                    (self.emit)(&SimEvent {
                        time,
                        object_id: id,
                        kind: EventKind::Circle,
                        part_index: None,
                        part_hit: None,
                        judgement: Some(judgement),
                        score_delta,
                        combo_before,
                        combo_after: self.stats.current_combo,
                    });
                }
            }
            TimelineEvent::Map(event) => match event.kind {
                MapEventKind::CircleDeadline if !self.resolved[event.object_id.0 as usize] => {
                    let combo_before = self.stats.current_combo;
                    let score_delta = apply_circle(
                        self.stats,
                        Judgement::Miss,
                        self.map.difficulty_multiplier(),
                    )?;
                    self.resolved[event.object_id.0 as usize] = true;
                    (self.emit)(&SimEvent {
                        time,
                        object_id: event.object_id,
                        kind: EventKind::Circle,
                        part_index: None,
                        part_hit: None,
                        judgement: Some(Judgement::Miss),
                        score_delta,
                        combo_before,
                        combo_after: self.stats.current_combo,
                    });
                }
                MapEventKind::SliderStart
                | MapEventKind::SliderTick
                | MapEventKind::SliderRepeat
                | MapEventKind::SliderTailCheck => {
                    if let Some(Some(slider)) = self.sliders.get_mut(event.object_id.0 as usize) {
                        slider.sample_for_map_events(
                            time,
                            *self.cursor,
                            *self.buttons,
                            self.map.visibility().radius,
                            std::slice::from_ref(&event),
                        )?;
                        if event.kind == MapEventKind::SliderTick
                            && let Some(index) = event.part_index
                        {
                            let combo_before = self.stats.current_combo;
                            let score_before = self.stats.score;
                            if let Some(hit) = slider.tick(index, self.stats)? {
                                (self.emit)(&SimEvent {
                                    time,
                                    object_id: event.object_id,
                                    kind: EventKind::SliderTick,
                                    part_index: Some(index),
                                    part_hit: Some(hit),
                                    judgement: None,
                                    score_delta: self.stats.score - score_before,
                                    combo_before,
                                    combo_after: self.stats.current_combo,
                                });
                            }
                            debug_assert!(self.stats.score >= score_before);
                            debug_assert!(
                                self.stats.current_combo >= combo_before
                                    || self.stats.current_combo == 0
                            );
                        }
                    }
                }
                _ => {}
            },
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::RoseaError,
        model::{Judgement, ObjectId, Point, ReplayInput, ReplayRecord, TimeMs},
        parse::osu,
        prepare::prepare_map,
        replay::normalize,
        sim::{EventKind, SimEvent, Simulator},
    };

    fn replay(records: &[(i64, f32, u32)]) -> crate::replay::ReplayTimeline {
        normalize(ReplayInput {
            records: records
                .iter()
                .enumerate()
                .map(|(index, &(delta_ms, x, keys))| ReplayRecord {
                    index: index as u32,
                    delta_ms,
                    cursor: Point { x, y: 0.0 },
                    keys,
                })
                .collect(),
            seed: None,
        })
        .unwrap()
        .value
    }

    #[test]
    fn finish_processes_deadlines_after_replay_and_emits_remaining_results() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0\n100,0,1200.25,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1000, 0.0, 1)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        assert_eq!(sim.end_time().get(), 1320.25);
        sim.advance_to(TimeMs::new(1000.0).unwrap()).unwrap();
        let mut events = Vec::new();
        let stats = sim.finish_with(&mut |event| events.push(*event)).unwrap();
        assert_eq!(stats.counts.count_300, 1);
        assert_eq!(stats.counts.count_miss, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].time.get(), 1320.25);
        assert_eq!(events[0].object_id, ObjectId(1));
        assert_eq!(events[0].judgement, Some(Judgement::Miss));
        assert_eq!(
            Simulator::new(&map, &replay).unwrap().finish().unwrap(),
            stats
        );
    }

    #[test]
    fn end_time_includes_late_replay_and_finish_accepts_later_advance() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(2000, 0.0, 1)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        assert_eq!(sim.end_time().get(), 2000.0);
        sim.advance_to(TimeMs::new(3000.0).unwrap()).unwrap();
        assert_eq!(sim.finish().unwrap().counts.count_miss, 1);
    }

    #[test]
    fn finish_rejects_missing_results_and_prior_score_error() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1000, 0.0, 1)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        sim.advance_to(TimeMs::new(2000.0).unwrap()).unwrap();
        sim.stats.counts.count_300 = 0;
        assert_eq!(
            sim.finish(),
            Err(RoseaError::IncompleteSimulation {
                expected: 1,
                actual: 0
            })
        );
        let mut sim = Simulator::new(&map, &replay).unwrap();
        sim.stats.score = u64::MAX;
        assert_eq!(
            sim.advance_to(TimeMs::new(1000.0).unwrap()),
            Err(RoseaError::ScoreOverflow)
        );
        assert_eq!(sim.finish(), Err(RoseaError::ScoreOverflow));
    }

    #[test]
    fn journal_records_circle_results_without_changing_stats() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0\n100,0,1000,1,0\n200,0,1000,1,0\n300,0,1100,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1000, 0.0, 1), (0, 100.0, 3), (0, 200.0, 0), (0, 200.0, 1)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        let mut plain = Simulator::new(&map, &replay).unwrap();
        let mut events = Vec::new();
        for time in [1000.0, 1000.0, 1220.0] {
            sim.advance_with(TimeMs::new(time).unwrap(), &mut |event| events.push(*event))
                .unwrap();
            plain.advance_to(TimeMs::new(time).unwrap()).unwrap();
            assert_eq!(sim.stats(), plain.stats());
        }
        let event = |time, id, judgement, score_delta, combo_before, combo_after| SimEvent {
            time: TimeMs::new(time).unwrap(),
            object_id: ObjectId(id),
            kind: EventKind::Circle,
            part_index: None,
            part_hit: None,
            judgement: Some(judgement),
            score_delta,
            combo_before,
            combo_after,
        };
        assert_eq!(
            events,
            [
                event(1000.0, 0, Judgement::Great, 300, 0, 1),
                event(1000.0, 1, Judgement::Great, 300, 1, 2),
                event(
                    1000.0,
                    2,
                    Judgement::Great,
                    300 + 12 * u64::from(map.difficulty_multiplier()),
                    2,
                    3
                ),
                event(1220.0, 3, Judgement::Miss, 0, 3, 0),
            ]
        );
    }

    #[test]
    fn failed_score_does_not_emit_result() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1000, 0.0, 1)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        sim.stats.score = u64::MAX;
        let mut events = Vec::new();
        assert_eq!(
            sim.advance_with(TimeMs::new(1000.0).unwrap(), &mut |event| events
                .push(*event)),
            Err(RoseaError::ScoreOverflow)
        );
        assert!(events.is_empty());
        assert_eq!(
            sim.advance_to(TimeMs::new(1001.0).unwrap()),
            Err(RoseaError::ScoreOverflow)
        );
    }

    #[test]
    fn presses_and_deadlines_update_stats_once() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0\n100,0,1000,1,0\n200,0,1200,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1000, 0.0, 1), (0, 100.0, 3), (200, 200.0, 3)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        sim.advance_to(TimeMs::new(1000.0).unwrap()).unwrap();
        assert_eq!(sim.stats().counts.count_300, 2);
        assert_eq!(sim.stats().score, 600);
        assert_eq!(sim.stats().current_combo, 2);
        sim.advance_to(TimeMs::new(1320.0).unwrap()).unwrap();
        assert_eq!(sim.stats().counts.count_miss, 1);
        assert_eq!(sim.stats().current_combo, 0);
        assert_eq!(sim.stats().max_combo, 2);
        let stats = sim.stats();
        sim.advance_to(TimeMs::new(1320.0).unwrap()).unwrap();
        assert_eq!(sim.stats(), stats);
        assert_eq!(
            sim.advance_to(TimeMs::new(1319.0).unwrap()),
            Err(RoseaError::TimeWentBackwards)
        );
    }

    #[test]
    fn inaccessible_earlier_circle_does_not_block_and_eof_does_not_hide_misses() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0\n100,0,1010,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1010, 100.0, 1)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        sim.advance_to(TimeMs::new(1010.0).unwrap()).unwrap();
        assert_eq!(sim.stats().counts.count_300, 1);
        sim.advance_to(TimeMs::new(1130.0).unwrap()).unwrap();
        assert_eq!(sim.stats().counts.count_miss, 1);
    }

    #[test]
    fn two_fronts_choose_earlier_circle_then_recheck_candidates() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n100,0,1000,1,0\n100,0,950,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let single = replay(&[(1000, 100.0, 1)]);
        let mut sim = Simulator::new(&map, &single).unwrap();
        sim.advance_to(TimeMs::new(1000.0).unwrap()).unwrap();
        assert_eq!(sim.stats().counts.count_100, 1);
        assert_eq!(sim.stats().counts.count_300, 0);
        let replay = replay(&[(1000, 100.0, 3)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        sim.advance_to(TimeMs::new(1000.0).unwrap()).unwrap();
        assert_eq!(sim.stats().counts.count_100, 1);
        assert_eq!(sim.stats().counts.count_300, 1);
        assert_eq!(sim.stats().current_combo, 2);
        assert_eq!(sim.stats().score, 400);
    }

    #[test]
    fn click_on_strict_outer_boundary_is_a_miss() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,1000,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1120, 0.0, 1)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        sim.advance_to(TimeMs::new(1120.0).unwrap()).unwrap();
        assert_eq!(sim.stats().counts.count_miss, 1);
        assert_eq!(sim.stats().score, 0);
    }

    #[test]
    fn unsupported_objects_are_rejected() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n256,192,1000,8,0,2000";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1000, 0.0, 0)]);
        assert!(matches!(
            Simulator::new(&map, &replay),
            Err(RoseaError::UnsupportedSimulationObject { .. })
        ));
    }

    #[test]
    fn slider_ticks_run_in_timeline_after_input_and_emit_part_results() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n0,0,1000,2,0,L|300:0,1,300";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let replay = replay(&[(1000, 0.0, 1), (500, 100.0, 1), (500, 300.0, 0)]);
        let mut sim = Simulator::new(&map, &replay).unwrap();
        let mut events = Vec::new();
        sim.advance_with(TimeMs::new(2000.0).unwrap(), &mut |event| {
            events.push(*event)
        })
        .unwrap();

        assert_eq!(sim.stats().score, 10);
        assert_eq!(sim.stats().current_combo, 0);
        assert_eq!(sim.stats().max_combo, 1);
        assert_eq!(sim.stats().counts.count_miss, 0);
        assert_eq!(
            events,
            [
                SimEvent {
                    time: TimeMs::new(1500.0).unwrap(),
                    object_id: ObjectId(0),
                    kind: EventKind::SliderTick,
                    part_index: Some(0),
                    part_hit: Some(true),
                    judgement: None,
                    score_delta: 10,
                    combo_before: 0,
                    combo_after: 1,
                },
                SimEvent {
                    time: TimeMs::new(2000.0).unwrap(),
                    object_id: ObjectId(0),
                    kind: EventKind::SliderTick,
                    part_index: Some(1),
                    part_hit: Some(false),
                    judgement: None,
                    score_delta: 0,
                    combo_before: 1,
                    combo_after: 0,
                },
            ]
        );
    }
}
