//! Прогон кругов по объединённой временной шкале.

use crate::error::RoseaError;
use crate::model::{Judgement, ObjectId, Stats, TimeMs};
use crate::prepare::{MapEventKind, PreparedMap, StackedObjectKind};
use crate::replay::ReplayTimeline;

use super::hit::{inside_radius, judge_click};
use super::input::InputEvent;
use super::score::apply_circle;
use super::timeline::{Timeline, TimelineEvent};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    Circle,
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

/// Частичный игровой прогон: пока поддерживаются только карты из кругов.
pub struct Simulator<'a> {
    map: &'a PreparedMap,
    timeline: Timeline<'a>,
    resolved: Vec<bool>,
    stats: Stats,
    failed: bool,
    end_time: TimeMs,
    current_time: Option<TimeMs>,
}

impl<'a> Simulator<'a> {
    pub fn new(map: &'a PreparedMap, input: &'a ReplayTimeline) -> Result<Self, RoseaError> {
        if let Some(object) = map
            .objects()
            .iter()
            .find(|object| object.kind() != StackedObjectKind::Circle)
        {
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
        let resolved = &mut self.resolved;
        let stats = &mut self.stats;
        let mut failed = false;
        self.timeline.advance_to(time, |time, event| {
            if failed {
                return;
            }
            let result = match event {
                TimelineEvent::Input(InputEvent::Press { cursor, .. }) => {
                    let candidate = map
                        .objects()
                        .iter()
                        .filter_map(|object| {
                            if resolved[object.object_id().0 as usize]
                                || !inside_radius(
                                    cursor,
                                    object.position(),
                                    map.visibility().radius,
                                )
                            {
                                return None;
                            }
                            let start = TimeMs::new(object.start_time()).ok()?;
                            let judgement =
                                judge_click(map.hit_windows(), start, time.get() as i64)?;
                            Some((object, judgement))
                        })
                        .min_by(|(a, _), (b, _)| {
                            a.start_time()
                                .total_cmp(&b.start_time())
                                .then(a.object_id().0.cmp(&b.object_id().0))
                        });
                    candidate.map(|(object, judgement)| (object.object_id(), judgement))
                }
                TimelineEvent::Map(event)
                    if event.kind == MapEventKind::CircleDeadline
                        && !resolved[event.object_id.0 as usize] =>
                {
                    Some((event.object_id, Judgement::Miss))
                }
                _ => None,
            };
            if let Some((id, judgement)) = result {
                let combo_before = stats.current_combo;
                match apply_circle(stats, judgement, map.difficulty_multiplier()) {
                    Ok(score_delta) => {
                        resolved[id.0 as usize] = true;
                        emit(&SimEvent {
                            time,
                            object_id: id,
                            kind: EventKind::Circle,
                            part_index: None,
                            part_hit: None,
                            judgement: Some(judgement),
                            score_delta,
                            combo_before,
                            combo_after: stats.current_combo,
                        });
                    }
                    Err(_) => failed = true,
                }
            }
        })?;
        if failed {
            self.failed = true;
            return Err(RoseaError::ScoreOverflow);
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
}
