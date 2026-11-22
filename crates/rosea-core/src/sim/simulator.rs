//! Прогон кругов и частей слайдеров по объединённой временной шкале.

use crate::error::RoseaError;
use crate::model::{Judgement, ObjectId, Point, Stats, TimeMs};
use crate::prepare::{MapEvent, MapEventKind, PreparedMap, StackedObjectKind};
use crate::replay::{ButtonState, ReplayTimeline};

use super::hit::{inside_radius, judge_click};
use super::input::InputEvent;
use super::score::{apply_circle, apply_slider_result};
use super::slider::{SliderState, map_event_needs_tracking, result_for_parts};
use super::timeline::{Timeline, TimelineEvent, TimelineStep};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    Circle,
    SliderHead,
    SliderTick,
    SliderRepeat,
    SliderTail,
    SliderResult,
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

/// Частичный прогон кругов и частей слайдеров.
pub struct Simulator<'a> {
    map: &'a PreparedMap,
    replay: &'a ReplayTimeline,
    timeline: Timeline<'a>,
    resolved: Vec<bool>,
    sliders: Vec<Option<SliderState<'a>>>,
    active_sliders: Vec<ObjectId>,
    next_slider_index: usize,
    stats: Stats,
    failed: bool,
    end_time: TimeMs,
    current_time: Option<TimeMs>,
}

struct EventProcessor<'map, 'state, 'sim> {
    map: &'map PreparedMap,
    replay: &'map ReplayTimeline,
    resolved: &'state mut [bool],
    sliders: &'state mut [Option<SliderState<'sim>>],
    active_sliders: &'state mut Vec<ObjectId>,
    next_slider_index: &'state mut usize,
    record_buttons: ButtonState,
    stats: &'state mut Stats,
    emit: &'state mut dyn FnMut(&SimEvent),
    late_heads: Vec<ObjectId>,
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
        // ObjectId — плотный индекс исходной записи до сортировки карты.
        let mut sliders = Vec::new();
        sliders.resize_with(map.objects().len(), || None);
        for object in map.objects() {
            sliders[object.object_id().0 as usize] = SliderState::new(object);
        }
        Ok(Self {
            map,
            replay: input,
            timeline: Timeline::new(map, input),
            resolved: vec![false; map.objects().len()],
            sliders,
            active_sliders: Vec::new(),
            next_slider_index: 0,
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
        let active_sliders = &mut self.active_sliders;
        let next_slider_index = &mut self.next_slider_index;
        let stats = &mut self.stats;
        let mut processor = EventProcessor {
            map,
            replay: self.replay,
            resolved,
            sliders,
            active_sliders,
            next_slider_index,
            record_buttons: ButtonState::default(),
            stats,
            emit,
            late_heads: Vec::new(),
        };
        let mut last_synced_time = None;
        let mut failure = None;
        timeline.advance_grouped(time, |time, step| {
            if failure.is_some() {
                return;
            }
            if last_synced_time != Some(time) {
                processor.sync_active_sliders(time);
                last_synced_time = Some(time);
            }
            failure = match step {
                TimelineStep::Input(event) => processor.process(time, TimelineEvent::Input(event)),
                TimelineStep::Map(events) => processor.process_map_group(time, events),
            }
            .err();
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
    /// Поддерживает упорядоченный список слайдеров, активных в момент `time`.
    ///
    /// В момент начала слайдер уже доступен фазе ввода той же временной группы.
    fn sync_active_sliders(&mut self, time: TimeMs) {
        let current = time.get();
        self.active_sliders.retain(|&id| {
            let Some(Some(slider)) = self.sliders.get_mut(id.0 as usize) else {
                return false;
            };
            if slider.end_time() < current {
                slider.clear_tracking();
                false
            } else {
                true
            }
        });
        let objects = self.map.objects();
        while *self.next_slider_index < objects.len() {
            let object = &objects[*self.next_slider_index];
            if object.start_time() > current {
                break;
            }
            *self.next_slider_index += 1;
            let Some(slider) = object.slider() else {
                continue;
            };
            if slider.timing().end_time < current {
                continue;
            }
            let id = object.object_id();
            if let Err(pos) = self
                .active_sliders
                .binary_search_by_key(&id.0, |item| item.0)
            {
                self.active_sliders.insert(pos, id);
            }
        }
    }

    fn process_map_group(&mut self, time: TimeMs, events: &[MapEvent]) -> Result<(), RoseaError> {
        // Все части группы используют ввод после завершения его фазы.
        let mut slider_events = events
            .iter()
            .filter(|event| map_event_needs_tracking(event.kind))
            .peekable();
        if slider_events.peek().is_some() {
            let cursor = self.replay.cursor_at(time);
            let buttons = self.replay.buttons_at(time);
            let radius = self.map.visibility().radius;
            for event in slider_events {
                if let Some(Some(slider)) = self.sliders.get_mut(event.object_id.0 as usize) {
                    slider.sample_at_map_time(time, cursor, buttons, radius)?;
                }
            }
        }

        self.late_heads.sort_by_key(|id| id.0);
        let mut late = std::mem::take(&mut self.late_heads).into_iter().peekable();
        for &event in events {
            if matches!(
                event.kind,
                MapEventKind::CircleDeadline
                    | MapEventKind::SliderHeadDeadline
                    | MapEventKind::SliderEnd
                    | MapEventKind::SpinnerEnd
            ) {
                while late.peek().is_some_and(|id| id.0 < event.object_id.0) {
                    if let Some(id) = late.next() {
                        self.finalize_slider(time, id)?;
                    }
                }
            }
            self.process(time, TimelineEvent::Map(event))?;
        }
        for id in late {
            self.finalize_slider(time, id)?;
        }
        Ok(())
    }

    fn press(&mut self, time: TimeMs, cursor: Point) -> Result<(), RoseaError> {
        let objects = self.map.objects();
        let windows = self.map.hit_windows();
        let radius = self.map.visibility().radius;
        let click = time.get();
        let click_ms = click as i64;
        let w50 = windows.meh as f64;
        // Объекты отсортированы по времени начала и исходному ObjectId.
        let first = objects.partition_point(|object| click - object.start_time() >= w50);
        let candidate = objects[first..]
            .iter()
            .take_while(|object| object.start_time() - click < w50)
            .find_map(|object| {
                let id = object.object_id().0 as usize;
                if self.resolved[id]
                    || self.sliders[id]
                        .as_ref()
                        .is_some_and(|slider| !slider.head_is_pending())
                    || !inside_radius(cursor, object.position(), radius)
                {
                    return None;
                }
                let start = TimeMs::new(object.start_time()).ok()?;
                let judgement = judge_click(windows, start, click_ms)?;
                Some((object.object_id(), judgement))
            });
        let Some((id, judgement)) = candidate else {
            return Ok(());
        };

        let combo_before = self.stats.current_combo;
        let score_before = self.stats.score;
        let index = id.0 as usize;
        let (kind, judgement, part_hit) = if let Some(slider) = &mut self.sliders[index] {
            if !slider.hit_head(
                click_ms,
                cursor,
                self.record_buttons,
                windows,
                radius,
                self.stats,
            )? {
                return Ok(());
            }
            if time.get() >= slider.end_time() {
                self.late_heads.push(id);
            }
            (EventKind::SliderHead, None, Some(true))
        } else {
            apply_circle(self.stats, judgement, self.map.difficulty_multiplier())?;
            self.resolved[index] = true;
            (EventKind::Circle, Some(judgement), None)
        };
        (self.emit)(&SimEvent {
            time,
            object_id: id,
            kind,
            part_index: None,
            part_hit,
            judgement,
            score_delta: self.stats.score - score_before,
            combo_before,
            combo_after: self.stats.current_combo,
        });
        Ok(())
    }

    fn process(&mut self, time: TimeMs, event: TimelineEvent) -> Result<(), RoseaError> {
        match event {
            TimelineEvent::Input(InputEvent::Record { buttons, .. }) => {
                self.record_buttons = buttons;
            }
            TimelineEvent::Input(
                input @ (InputEvent::RecordEnd { .. } | InputEvent::End { .. }),
            ) => {
                let radius = self.map.visibility().radius;
                for &id in self.active_sliders.iter() {
                    if let Some(Some(slider)) = self.sliders.get_mut(id.0 as usize) {
                        slider.observe_input_event(time, input, radius)?;
                    }
                }
            }
            TimelineEvent::Input(InputEvent::Press { cursor, .. }) => {
                self.press(time, cursor)?;
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
                MapEventKind::SliderTick
                | MapEventKind::SliderRepeat
                | MapEventKind::SliderTailCheck => {
                    if let Some(Some(slider)) = self.sliders.get_mut(event.object_id.0 as usize)
                        && let Some(index) = event.part_index
                    {
                        let combo_before = self.stats.current_combo;
                        let score_before = self.stats.score;
                        let (kind, result) = match event.kind {
                            MapEventKind::SliderTick => {
                                (EventKind::SliderTick, slider.tick(index, self.stats)?)
                            }
                            MapEventKind::SliderRepeat => {
                                (EventKind::SliderRepeat, slider.repeat(index, self.stats)?)
                            }
                            MapEventKind::SliderTailCheck => {
                                (EventKind::SliderTail, slider.tail_check(index, self.stats)?)
                            }
                            _ => return Ok(()),
                        };
                        if let Some(hit) = result {
                            (self.emit)(&SimEvent {
                                time,
                                object_id: event.object_id,
                                kind,
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
                MapEventKind::SliderHeadDeadline => {
                    if let Some(Some(slider)) = self.sliders.get_mut(event.object_id.0 as usize) {
                        let combo_before = self.stats.current_combo;
                        let windows = self.map.hit_windows();
                        if slider.timeout_head(time, windows, self.stats) {
                            (self.emit)(&SimEvent {
                                time,
                                object_id: event.object_id,
                                kind: EventKind::SliderHead,
                                part_index: None,
                                part_hit: Some(false),
                                judgement: None,
                                score_delta: 0,
                                combo_before,
                                combo_after: self.stats.current_combo,
                            });
                            debug_assert_eq!(self.stats.current_combo, 0);
                        }
                    }
                    self.finalize_slider(time, event.object_id)?;
                }
                MapEventKind::SliderEnd => {
                    if let Ok(pos) = self
                        .active_sliders
                        .binary_search_by_key(&event.object_id.0, |item| item.0)
                    {
                        self.active_sliders.remove(pos);
                    }
                    if let Some(Some(slider)) = self.sliders.get_mut(event.object_id.0 as usize) {
                        slider.clear_tracking();
                    }
                    self.finalize_slider(time, event.object_id)?;
                }
                _ => {}
            },
        }
        Ok(())
    }

    fn finalize_slider(&mut self, time: TimeMs, id: ObjectId) -> Result<(), RoseaError> {
        let index = id.0 as usize;
        let Some(Some(slider)) = self.sliders.get(index) else {
            return Ok(());
        };
        if self.resolved[index] || slider.head_is_pending() || time.get() < slider.end_time() {
            return Ok(());
        }
        let judgement = result_for_parts(slider.successful_parts(), slider.total_parts());
        let combo_before = self.stats.current_combo;
        let score_delta =
            apply_slider_result(self.stats, judgement, self.map.difficulty_multiplier())?;
        self.resolved[index] = true;
        (self.emit)(&SimEvent {
            time,
            object_id: id,
            kind: EventKind::SliderResult,
            part_index: None,
            part_hit: None,
            judgement: Some(judgement),
            score_delta,
            combo_before,
            combo_after: self.stats.current_combo,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::RoseaError,
        model::{Judgement, ObjectId, Point, ReplayInput, ReplayRecord, Stats, TimeMs},
        parse::osu,
        prepare::{PreparedMap, prepare_map},
        replay::{ReplayTimeline, normalize},
        sim::{EventKind, SimEvent, Simulator},
    };

    const ONE_TICK_SLIDER: &str = "0,0,1000,2,0,L|200:0,1,200";
    const ONE_REPEAT_SLIDER: &str = "0,0,1000,2,0,L|200:0,2,200";

    fn replay(records: &[(i64, f32, u32)]) -> ReplayTimeline {
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

    fn slider_map(objects: &str) -> PreparedMap {
        let map = format!(
            "osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,500,4,0,0,100,1,0\n[HitObjects]\n{objects}"
        );
        prepare_map(&osu(map.as_bytes()).unwrap().value).unwrap()
    }

    fn run_with_journal(
        map: &PreparedMap,
        replay: &ReplayTimeline,
        times: impl IntoIterator<Item = f64>,
    ) -> (Stats, Vec<SimEvent>) {
        let mut sim = Simulator::new(map, replay).unwrap();
        let mut events = Vec::new();
        for time in times {
            sim.advance_with(TimeMs::new(time).unwrap(), &mut |event| events.push(*event))
                .unwrap();
        }
        (sim.stats(), events)
    }

    fn head_event(time: f64, object_id: u32, combo_before: u32) -> SimEvent {
        SimEvent {
            time: TimeMs::new(time).unwrap(),
            object_id: ObjectId(object_id),
            kind: EventKind::SliderHead,
            part_index: None,
            part_hit: Some(true),
            judgement: None,
            score_delta: 30,
            combo_before,
            combo_after: combo_before + 1,
        }
    }

    fn head_miss_event(time: f64, object_id: u32, combo_before: u32) -> SimEvent {
        SimEvent {
            time: TimeMs::new(time).unwrap(),
            object_id: ObjectId(object_id),
            kind: EventKind::SliderHead,
            part_index: None,
            part_hit: Some(false),
            judgement: None,
            score_delta: 0,
            combo_before,
            combo_after: 0,
        }
    }

    fn tick_event(
        time: f64,
        object_id: u32,
        index: u32,
        hit: bool,
        score_delta: u64,
        combo: (u32, u32),
    ) -> SimEvent {
        SimEvent {
            time: TimeMs::new(time).unwrap(),
            object_id: ObjectId(object_id),
            kind: EventKind::SliderTick,
            part_index: Some(index),
            part_hit: Some(hit),
            judgement: None,
            score_delta,
            combo_before: combo.0,
            combo_after: combo.1,
        }
    }

    fn tail_event(
        time: f64,
        object_id: u32,
        index: u32,
        hit: bool,
        score_delta: u64,
        combo: (u32, u32),
    ) -> SimEvent {
        SimEvent {
            time: TimeMs::new(time).unwrap(),
            object_id: ObjectId(object_id),
            kind: EventKind::SliderTail,
            part_index: Some(index),
            part_hit: Some(hit),
            judgement: None,
            score_delta,
            combo_before: combo.0,
            combo_after: combo.1,
        }
    }

    fn result_event(time: f64, id: u32, judgement: Judgement, score: u64, combo: u32) -> SimEvent {
        SimEvent {
            time: TimeMs::new(time).unwrap(),
            object_id: ObjectId(id),
            kind: EventKind::SliderResult,
            part_index: None,
            part_hit: None,
            judgement: Some(judgement),
            score_delta: score,
            combo_before: combo,
            combo_after: combo,
        }
    }

    #[test]
    fn slider_results_finish_once_after_parts_or_late_head() {
        let map = slider_map(ONE_TICK_SLIDER);
        let input = replay(&[(1000, 0.0, 1), (1000, 200.0, 1)]);
        let mut sim = Simulator::new(&map, &input).unwrap();
        let mut events = Vec::new();
        sim.advance_with(TimeMs::new(2000.0).unwrap(), &mut |event| {
            events.push(*event)
        })
        .unwrap();
        let result = events.last().unwrap();
        assert_eq!(
            *result,
            result_event(
                2000.0,
                0,
                Judgement::Great,
                300 + 24 * u64::from(map.difficulty_multiplier()),
                3
            )
        );
        let stats = sim.finish_with(&mut |event| events.push(*event)).unwrap();
        assert_eq!(stats.counts.count_300, 1);
        assert_eq!(stats.current_combo, 3);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == EventKind::SliderResult)
                .count(),
            1
        );
        assert_eq!(
            Simulator::new(&map, &input).unwrap().finish().unwrap(),
            stats
        );

        let short = slider_map("0,0,1000,2,0,L|100:0,1,0");
        let late = replay(&[(1050, 0.0, 1)]);
        let sim = Simulator::new(&short, &late).unwrap();
        let mut events = Vec::new();
        let stats = sim.finish_with(&mut |event| events.push(*event)).unwrap();
        assert_eq!(events.last().unwrap().time.get(), 1050.0);
        assert_eq!(events.last().unwrap().kind, EventKind::SliderResult);
        assert_eq!(stats.counts.count_100, 1);
    }

    #[test]
    fn slider_state_uses_source_id_on_unsorted_maps() {
        let circle = "300,0,4000,1,0";
        let input = replay(&[
            (1000, 0.0, 1),
            (500, 100.0, 1),
            (500, 200.0, 0),
            (2000, 300.0, 1),
        ]);
        let ordered = slider_map(&format!("{ONE_TICK_SLIDER}\n{circle}"));
        let reversed = slider_map(&format!("{circle}\n{ONE_TICK_SLIDER}"));
        let (ordered_stats, ordered_events) = run_with_journal(&ordered, &input, [4120.0]);
        let (reversed_stats, reversed_events) = run_with_journal(&reversed, &input, [4120.0]);
        let circle_score = 300 + 24 * u64::from(ordered.difficulty_multiplier());
        assert_eq!(
            ordered_events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (1, 2)),
                tail_event(1964.0, 0, 1, true, 30, (2, 3)),
                result_event(2000.0, 0, Judgement::Great, circle_score, 3),
                SimEvent {
                    time: TimeMs::new(4000.0).unwrap(),
                    object_id: ObjectId(1),
                    kind: EventKind::Circle,
                    part_index: None,
                    part_hit: None,
                    judgement: Some(Judgement::Great),
                    score_delta: circle_score,
                    combo_before: 3,
                    combo_after: 4,
                },
            ]
        );
        assert_eq!(ordered_stats.score, 70 + 2 * circle_score);
        let expected_reversed = [
            SimEvent {
                object_id: ObjectId(1),
                ..ordered_events[0]
            },
            SimEvent {
                object_id: ObjectId(1),
                ..ordered_events[1]
            },
            SimEvent {
                object_id: ObjectId(1),
                ..ordered_events[2]
            },
            SimEvent {
                object_id: ObjectId(1),
                ..ordered_events[3]
            },
            SimEvent {
                object_id: ObjectId(0),
                ..ordered_events[4]
            },
        ];
        assert_eq!(reversed_events, expected_reversed);
        assert_eq!(reversed_stats, ordered_stats);
    }

    #[test]
    fn slider_tick_matches_with_or_without_a_record_at_tick_time() {
        let map = slider_map(ONE_TICK_SLIDER);
        let sparse = replay(&[(1000, 0.0, 1), (1000, 200.0, 1)]);
        let dense = replay(&[(1000, 0.0, 1), (500, 100.0, 1), (500, 200.0, 1)]);
        let expected = run_with_journal(&map, &dense, [2000.0]);
        assert_eq!(
            expected.1,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (1, 2)),
                tail_event(1964.0, 0, 1, true, 30, (2, 3)),
                result_event(
                    2000.0,
                    0,
                    Judgement::Great,
                    300 + 24 * u64::from(map.difficulty_multiplier()),
                    3
                ),
            ]
        );
        assert_eq!(
            expected.0.score,
            370 + 24 * u64::from(map.difficulty_multiplier())
        );
        assert_eq!(run_with_journal(&map, &sparse, [2000.0]), expected);
    }

    #[test]
    fn slider_ticks_use_the_correct_endpoints_of_equal_time_input_groups() {
        let map = slider_map("0,0,1000,2,0,L|300:0,1,300");
        let input = replay(&[
            (1000, 300.0, 1),
            (0, 0.0, 1),
            (1000, 200.0, 1),
            (0, 400.0, 1),
            (500, 300.0, 1),
        ]);
        let (stats, events) = run_with_journal(&map, &input, [2500.0]);
        assert_eq!(
            events,
            [
                head_miss_event(1120.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (0, 1)),
                tick_event(2000.0, 0, 1, false, 0, (1, 0)),
                tail_event(2464.0, 0, 2, true, 30, (0, 1)),
                result_event(2500.0, 0, Judgement::Ok, 100, 1),
            ]
        );
        assert_eq!(stats.score, 140);
    }

    #[test]
    fn eof_releases_buttons_after_last_press_and_before_map_parts() {
        let map = slider_map("0,0,1000,2,0,L|300:0,2,300\n200,0,2000,1,0");
        let input = replay(&[(1000, 0.0, 1), (1000, 200.0, 3)]);
        let (stats, events) = run_with_journal(&map, &input, [4000.0]);
        let circle_score = 300 + 12 * u64::from(map.difficulty_multiplier());
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (1, 2)),
                SimEvent {
                    time: TimeMs::new(2000.0).unwrap(),
                    object_id: ObjectId(1),
                    kind: EventKind::Circle,
                    part_index: None,
                    part_hit: None,
                    judgement: Some(Judgement::Great),
                    score_delta: circle_score,
                    combo_before: 2,
                    combo_after: 3,
                },
                tick_event(2000.0, 0, 1, false, 0, (3, 0)),
                SimEvent {
                    time: TimeMs::new(2500.0).unwrap(),
                    object_id: ObjectId(0),
                    kind: EventKind::SliderRepeat,
                    part_index: Some(2),
                    part_hit: Some(false),
                    judgement: None,
                    score_delta: 0,
                    combo_before: 0,
                    combo_after: 0,
                },
                tick_event(3000.0, 0, 3, false, 0, (0, 0)),
                tick_event(3500.0, 0, 4, false, 0, (0, 0)),
                tail_event(3964.0, 0, 5, false, 0, (0, 0)),
                result_event(4000.0, 0, Judgement::Meh, 50, 0),
            ]
        );
        assert_eq!(stats.score, 90 + circle_score);
        assert_eq!(stats.max_combo, 3);
    }

    #[test]
    fn slider_tracking_is_independent_of_advance_frequency() {
        let map = slider_map(ONE_TICK_SLIDER);
        // Курсор проходит через шарик между событиями. К моменту тика он
        // находится в радиусе удержания, но уже вне радиуса захвата.
        let input = replay(&[(1000, -100.0, 1), (1000, 400.0, 1)]);
        let expected = run_with_journal(&map, &input, [2000.0]);
        assert_eq!(
            expected.1,
            [
                head_miss_event(1120.0, 0, 0),
                tick_event(1500.0, 0, 0, false, 0, (0, 0)),
                tail_event(1964.0, 0, 1, false, 0, (0, 0)),
                result_event(2000.0, 0, Judgement::Miss, 0, 0),
            ]
        );
        for fps in [30.0, 60.0, 144.0] {
            let times = (0..)
                .map(|frame| f64::from(frame) * 1000.0 / fps)
                .take_while(|&time| time < 2000.0)
                .chain([2000.0]);
            assert_eq!(run_with_journal(&map, &input, times), expected, "{fps} FPS");
        }
        assert_eq!(
            run_with_journal(
                &map,
                &input,
                [1000.0, 1333.25, 1499.5, 1500.0, 1500.0, 2000.0]
            ),
            expected
        );
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
    fn slider_head_competes_with_circle_and_scores_only_once() {
        let map = slider_map(&format!("0,0,1040,1,0\n{ONE_TICK_SLIDER}"));
        let input = replay(&[
            (1000, 0.0, 1),
            (20, 0.0, 0),
            (20, 0.0, 1),
            (10, 0.0, 0),
            (10, 0.0, 1),
        ]);
        let (stats, events) = run_with_journal(&map, &input, [1000.0, 1040.0, 1060.0]);
        assert_eq!(
            events,
            [
                head_event(1000.0, 1, 0),
                SimEvent {
                    time: TimeMs::new(1040.0).unwrap(),
                    object_id: ObjectId(0),
                    kind: EventKind::Circle,
                    part_index: None,
                    part_hit: None,
                    judgement: Some(Judgement::Great),
                    score_delta: 300,
                    combo_before: 1,
                    combo_after: 2,
                },
            ]
        );
        assert_eq!(stats.score, 330);
        assert_eq!(stats.current_combo, 2);
        assert_eq!(stats.max_combo, 2);
        assert_eq!(stats.counts.count_300, 1);
        assert_eq!(stats.counts.count_100, 0);
        assert_eq!(stats.counts.count_50, 0);
        assert_eq!(stats.counts.count_miss, 0);
    }

    #[test]
    fn slider_head_uses_strict_fractional_window_and_expanded_capture() {
        // Курсор попадает в радиус головы, но уже дальше 32 px от шарика.
        let input = replay(&[(1120, -20.0, 2), (500, 70.0, 0)]);
        let fractional = slider_map("0,0,1000.25,2,0,L|200:0,1,200");
        let (stats, events) = run_with_journal(&fractional, &input, [1120.0, 1500.25]);
        assert_eq!(
            events,
            [
                head_event(1120.0, 0, 0),
                tick_event(1500.25, 0, 0, true, 10, (1, 2)),
            ]
        );
        assert_eq!(stats.score, 40);
        assert_eq!(stats.counts, Stats::default().counts);

        let integral = slider_map(ONE_TICK_SLIDER);
        let (stats, events) = run_with_journal(&integral, &input, [1500.25]);
        assert_eq!(
            events,
            [
                head_miss_event(1120.0, 0, 0),
                tick_event(1500.0, 0, 0, false, 0, (0, 0)),
            ]
        );
        assert_eq!(stats, Stats::default());
    }

    #[test]
    fn slider_head_timeout_resets_combo_without_counting_a_miss() {
        // Круг в 500 мс даёт комбо 1; слайдер в 1000 мс не получает клика.
        let map = slider_map(&format!("0,0,500,1,0\n{ONE_TICK_SLIDER}"));
        let input = replay(&[(500, 0.0, 1), (100, 0.0, 0)]);
        let (stats, events) = run_with_journal(&map, &input, [500.0, 1120.0, 1120.0, 1500.0]);
        let circle_score = 300;
        assert_eq!(
            events,
            [
                SimEvent {
                    time: TimeMs::new(500.0).unwrap(),
                    object_id: ObjectId(0),
                    kind: EventKind::Circle,
                    part_index: None,
                    part_hit: None,
                    judgement: Some(Judgement::Great),
                    score_delta: circle_score,
                    combo_before: 0,
                    combo_after: 1,
                },
                head_miss_event(1120.0, 1, 1),
                tick_event(1500.0, 1, 0, false, 0, (0, 0)),
            ]
        );
        // Срок головы сбросил current_combo в 0, но max_combo сохранился,
        // а count_miss не увеличился.
        assert_eq!(stats.score, circle_score);
        assert_eq!(stats.current_combo, 0);
        assert_eq!(stats.max_combo, 1);
        assert_eq!(stats.counts.count_300, 1);
        assert_eq!(stats.counts.count_100, 0);
        assert_eq!(stats.counts.count_50, 0);
        assert_eq!(stats.counts.count_miss, 0);
    }

    #[test]
    fn slider_head_timeout_allows_subsequent_body_capture() {
        // Пропуск головы слайдера не блокирует последующий захват шарика:
        // после таймаута нажатие и удержание на шарике дают успешные тики и хвост.
        let map = slider_map("0,0,1000,2,0,L|300:0,1,300");
        // Голова в 1000 мс (срок 1120 мс) пропускается.
        // В 1300 мс курсор встаёт на позицию шарика (60 px по X) с зажатой кнопкой.
        let input = replay(&[
            (1000, -100.0, 0),
            (300, 60.0, 1),
            (200, 100.0, 1),
            (500, 200.0, 1),
            (464, 292.8, 1),
            (36, 300.0, 1),
        ]);
        let (stats, events) = run_with_journal(&map, &input, [1120.0, 1500.0, 2000.0, 2464.0]);
        assert_eq!(
            events,
            [
                head_miss_event(1120.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (0, 1)),
                tick_event(2000.0, 0, 1, true, 10, (1, 2)),
                tail_event(2464.0, 0, 2, true, 30, (2, 3)),
            ]
        );
        assert_eq!(stats.score, 50);
        assert_eq!(stats.current_combo, 3);
        assert_eq!(stats.max_combo, 3);
        assert_eq!(stats.counts.count_miss, 0);
    }

    #[test]
    fn slider_head_timeout_checks_fractional_boundary() {
        let fractional = slider_map("0,0,1000.25,2,0,L|200:0,1,200");
        // Клик в 1120 мс: delta = 119.75 < 120 (успех), таймаут в 1120.25 не срабатывает.
        let hit_input = replay(&[(1120, 0.0, 1), (380, 100.0, 1), (500, 100.0, 1)]);
        let (hit_stats, hit_events) =
            run_with_journal(&fractional, &hit_input, [1120.0, 1120.25, 1500.25]);
        assert_eq!(
            hit_events,
            [
                head_event(1120.0, 0, 0),
                tick_event(1500.25, 0, 0, true, 10, (1, 2)),
            ]
        );
        assert_eq!(hit_stats.score, 40);
        assert_eq!(hit_stats.counts.count_miss, 0);

        // Клик в 1121 мс: delta = 120.75 >= 120 (вне окна), таймаут срабатывает в 1120.25 мс.
        let miss_input = replay(&[(1121, 0.0, 1), (379, 100.0, 1), (500, 100.0, 1)]);
        let (miss_stats, miss_events) =
            run_with_journal(&fractional, &miss_input, [1120.25, 1121.0, 1500.25]);
        assert_eq!(
            miss_events,
            [
                head_miss_event(1120.25, 0, 0),
                tick_event(1500.25, 0, 0, true, 10, (0, 1)),
            ]
        );
        assert_eq!(miss_stats.score, 10);
        assert_eq!(miss_stats.counts.count_miss, 0);
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

        assert_eq!(sim.stats().score, 40);
        assert_eq!(sim.stats().current_combo, 0);
        assert_eq!(sim.stats().max_combo, 2);
        assert_eq!(sim.stats().counts.count_miss, 0);
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (1, 2)),
                tick_event(2000.0, 0, 1, false, 0, (2, 0)),
            ]
        );
    }

    #[test]
    fn slider_repeat_hit_follows_simultaneous_tick_and_updates_stats() {
        let map = slider_map(&format!(
            "{ONE_REPEAT_SLIDER}\n100,0,1500,2,0,L|300:0,1,200"
        ));
        let input = replay(&[(1000, 0.0, 1), (1500, 300.0, 1)]);
        let (stats, events) = run_with_journal(&map, &input, [2000.0]);
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (1, 2)),
                head_miss_event(1620.0, 1, 2),
                tick_event(2000.0, 1, 0, true, 10, (0, 1)),
                SimEvent {
                    time: TimeMs::new(2000.0).unwrap(),
                    object_id: ObjectId(0),
                    kind: EventKind::SliderRepeat,
                    part_index: Some(1),
                    part_hit: Some(true),
                    judgement: None,
                    score_delta: 30,
                    combo_before: 1,
                    combo_after: 2,
                },
            ]
        );
        assert_eq!(stats.score, 80);
        assert_eq!(stats.current_combo, 2);
        assert_eq!(stats.max_combo, 2);
        assert_eq!(stats.counts, Stats::default().counts);

        let mut plain = Simulator::new(&map, &input).unwrap();
        plain.advance_to(TimeMs::new(2000.0).unwrap()).unwrap();
        assert_eq!(plain.stats(), stats);

        for fps in [30.0, 60.0, 144.0] {
            let times = (0..)
                .map(|frame| f64::from(frame) * 1000.0 / fps)
                .take_while(|&time| time < 2000.0)
                .chain([2000.0, 2000.0]);
            let (frame_stats, frame_events) = run_with_journal(&map, &input, times);
            assert_eq!(frame_stats, stats, "{fps} FPS");
            assert_eq!(frame_events, events, "{fps} FPS");
        }
    }

    #[test]
    fn slider_repeat_miss_after_release_resets_combo_without_counting_a_miss() {
        let map = slider_map(ONE_REPEAT_SLIDER);
        let input = replay(&[(1000, 0.0, 1), (1000, 200.0, 0), (500, 100.0, 0)]);
        let (stats, events) = run_with_journal(&map, &input, [1999.5, 2000.0]);
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (1, 2)),
                SimEvent {
                    time: TimeMs::new(2000.0).unwrap(),
                    object_id: ObjectId(0),
                    kind: EventKind::SliderRepeat,
                    part_index: Some(1),
                    part_hit: Some(false),
                    judgement: None,
                    score_delta: 0,
                    combo_before: 2,
                    combo_after: 0,
                },
            ]
        );
        assert_eq!(stats.score, 40);
        assert_eq!(stats.current_combo, 0);
        assert_eq!(stats.max_combo, 2);
        assert_eq!(stats.counts.count_miss, 0);
    }

    #[test]
    fn slider_tail_hit_uses_exact_prepared_time_and_follows_the_last_tick() {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n[TimingPoints]\n0,20,4,0,0,100,1,0\n[HitObjects]\n0,0,1000.25,2,0,L|200:0,2,200";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let input = replay(&[(1000, 0.0, 1), (40, 200.0, 1), (40, 0.0, 0)]);
        let mut sim = Simulator::new(&map, &input).unwrap();
        let mut events = Vec::new();

        sim.advance_with(TimeMs::new(1060.0).unwrap(), &mut |event| {
            events.push(*event)
        })
        .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], head_event(1000.0, 0, 0));
        assert_eq!(sim.stats().score, 70);
        assert_eq!(sim.stats().current_combo, 3);

        // В 1060.25 мс шарик около x=100, а конец обратного прохода в x=0.
        let tail_time = TimeMs::new(1060.25).unwrap();
        sim.advance_with(tail_time, &mut |event| events.push(*event))
            .unwrap();
        assert_eq!(
            &events[3..],
            [
                tick_event(1060.25, 0, 2, true, 10, (3, 4)),
                tail_event(1060.25, 0, 3, true, 30, (4, 5)),
            ]
        );
        let stats = sim.stats();
        assert_eq!(stats.score, 110);
        assert_eq!(stats.current_combo, 5);
        assert_eq!(stats.max_combo, 5);
        assert_eq!(stats.counts, Stats::default().counts);

        let after_end = TimeMs::new(1081.0).unwrap();
        sim.advance_with(tail_time, &mut |event| events.push(*event))
            .unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(sim.stats(), stats);
        sim.advance_with(after_end, &mut |event| events.push(*event))
            .unwrap();
        assert_eq!(events.len(), 6);
        assert_eq!(
            events[5],
            result_event(
                1080.25,
                0,
                Judgement::Great,
                300 + 48 * u64::from(map.difficulty_multiplier()),
                5
            )
        );
        let stats = sim.stats();
        let mut plain = Simulator::new(&map, &input).unwrap();
        plain.advance_to(after_end).unwrap();
        assert_eq!(plain.stats(), stats);
    }

    #[test]
    fn slider_tail_missed_at_release_preserves_combo_and_counts() {
        let map = slider_map(ONE_TICK_SLIDER);
        let input = replay(&[(1000, 0.0, 1), (964, 192.8, 0), (36, 200.0, 0)]);
        let (stats, events) = run_with_journal(&map, &input, [2000.0]);
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, true, 10, (1, 2)),
                tail_event(1964.0, 0, 1, false, 0, (2, 2)),
                result_event(
                    2000.0,
                    0,
                    Judgement::Ok,
                    100 + 4 * u64::from(map.difficulty_multiplier()),
                    2
                ),
            ]
        );
        assert_eq!(
            stats.score,
            140 + 4 * u64::from(map.difficulty_multiplier())
        );
        assert_eq!(stats.current_combo, 2);
        assert_eq!(stats.max_combo, 2);
        assert_eq!(stats.counts.count_100, 1);
    }

    #[test]
    fn simultaneous_slider_parts_use_each_objects_tracking_in_phase_order() {
        use EventKind::{SliderRepeat, SliderTail, SliderTick};

        let map = slider_map(concat!(
            "100,0,1500,2,0,L|100:0,2,0\n",
            "200,0,1000,2,0,L|400:0,1,200\n",
            "400,0,1500,2,0,L|400:0,2,0\n",
            "45,0,1100,2,0,L|245:0,1,200",
        ));
        let sparse = replay(&[(1000, 100.0, 1), (1000, 100.0, 0)]);
        let dense = replay(&[
            (1000, 100.0, 1),
            (500, 300.0, 1),
            (0, 100.0, 0),
            (0, 100.0, 1),
            (500, 100.0, 0),
        ]);
        let (stats, events) = run_with_journal(&map, &sparse, [1500.0]);
        assert_eq!(events[0], head_miss_event(1120.0, 1, 0));
        assert_eq!(events[1], head_miss_event(1220.0, 3, 0));
        assert!(events[2..].iter().all(|event| event.time.get() == 1500.0));
        let parts: Vec<_> = events[2..]
            .iter()
            .map(|event| {
                (
                    event.kind,
                    event.object_id.0,
                    event.part_index,
                    event.part_hit,
                    event.combo_after,
                )
            })
            .collect();
        assert_eq!(
            parts,
            [
                (SliderTick, 1, Some(0), Some(false), 0),
                (SliderRepeat, 0, Some(0), Some(true), 1),
                (SliderRepeat, 2, Some(0), Some(false), 0),
                (SliderTail, 0, Some(1), Some(true), 1),
                (SliderTail, 2, Some(1), Some(false), 1),
            ]
        );
        assert_eq!(stats.score, 60);
        let (dense_stats, dense_events) = run_with_journal(&map, &dense, [1499.5, 1500.0, 1500.0]);
        assert_eq!(dense_events[0], head_miss_event(1120.0, 1, 0));
        assert_eq!(dense_events[1], head_miss_event(1220.0, 3, 0));
        assert_eq!(dense_events[2], head_event(1500.0, 0, 0));
        assert_eq!(dense_events[3], tick_event(1500.0, 1, 0, false, 0, (1, 0)));
        assert_eq!(&dense_events[4..8], &events[3..]);
        assert_eq!(
            dense_events[8],
            result_event(1500.0, 0, Judgement::Great, 300, 1)
        );
        let mut expected_stats = stats;
        expected_stats.score = 390;
        expected_stats.counts.count_300 = 1;
        assert_eq!(dense_stats, expected_stats);

        // Слайдер 3 был в радиусе захвата в 1500 мс, но своего события не имел.
        // К его тику курсор остался только в расширенном радиусе удержания.
        let (_, later_events) = run_with_journal(&map, &sparse, [1500.0, 1600.0]);
        assert_eq!(
            &later_events[7..],
            [tick_event(1600.0, 3, 0, false, 0, (1, 0))]
        );
    }

    #[test]
    fn slider_tracking_updates_on_release_and_recapture_between_parts() {
        let map = slider_map("0,0,1000,2,0,L|300:0,1,300");
        // CS=5 даёт радиус захвата 32 и удержания 76.8.
        // Отпускание в 1250 мс срывает удержание, поэтому нажатие на расстоянии 50 не захватывает.
        // Возврат на шарик в 1750 мс заново захватывает его и позволяет пройти тик и хвост на 50 px.
        let input = replay(&[
            (1000, 0.0, 1),
            (250, 50.0, 0),
            (100, 120.0, 1),
            (150, 150.0, 1),
            (250, 150.0, 1),
            (250, 250.0, 1),
            (500, 350.0, 1),
        ]);
        let expected = run_with_journal(&map, &input, [2500.0]);
        assert_eq!(
            expected.1,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, false, 0, (1, 0)),
                tick_event(2000.0, 0, 1, true, 10, (0, 1)),
                tail_event(2464.0, 0, 2, true, 30, (1, 2)),
                result_event(
                    2500.0,
                    0,
                    Judgement::Ok,
                    100 + 4 * u64::from(map.difficulty_multiplier()),
                    2
                ),
            ]
        );
        assert_eq!(
            expected.0.score,
            170 + 4 * u64::from(map.difficulty_multiplier())
        );
        assert_eq!(expected.0.current_combo, 2);
        assert_eq!(expected.0.max_combo, 2);
        assert_eq!(expected.0.counts.count_100, 1);

        let mut plain = Simulator::new(&map, &input).unwrap();
        plain.advance_to(TimeMs::new(2500.0).unwrap()).unwrap();
        assert_eq!(plain.stats(), expected.0);

        for fps in [30.0, 60.0, 144.0] {
            let times = (0..)
                .map(|frame| f64::from(frame) * 1000.0 / fps)
                .take_while(|&time| time < 2500.0)
                .chain([2500.0]);
            assert_eq!(run_with_journal(&map, &input, times), expected, "{fps} FPS");
        }
    }

    #[test]
    fn slider_tracking_updates_after_each_equal_time_record_between_and_on_parts() {
        let map = slider_map("0,0,1000,2,0,L|300:0,1,300");

        // Несколько записей в одно время между частями:
        // 1250: отпускание + нажатие в зоне удержания -> срыв;
        // 1750: захват + выход в зону удержания -> сохранение;
        // 2250: выход за 2.4*r + возврат в зону удержания -> срыв.
        let between_parts = replay(&[
            (1000, 0.0, 1),
            (250, 50.0, 0),
            (0, 100.0, 1),
            (250, 150.0, 1),
            (250, 150.0, 1),
            (0, 200.0, 1),
            (250, 250.0, 1),
            (250, 350.0, 1),
            (0, 300.0, 1),
            (250, 350.0, 1),
        ]);
        let (stats, events) = run_with_journal(&map, &between_parts, [2500.0]);
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, false, 0, (1, 0)),
                tick_event(2000.0, 0, 1, true, 10, (0, 1)),
                tail_event(2464.0, 0, 2, false, 0, (1, 1)),
                result_event(2500.0, 0, Judgement::Ok, 100, 1),
            ]
        );
        assert_eq!(stats.score, 140);
        assert_eq!(stats.current_combo, 1);
        assert_eq!(stats.max_combo, 1);

        // Несколько записей в момент старта и в моменты тиков; до старта и после конца tracking = false.
        let on_parts = replay(&[
            (900, 0.0, 1),
            (100, 0.0, 1),
            (0, 50.0, 1),
            (500, 100.0, 0),
            (0, 150.0, 1),
            (500, 200.0, 1),
            (0, 250.0, 1),
            (500, 350.0, 1),
        ]);
        let mut sim = Simulator::new(&map, &on_parts).unwrap();
        let mut on_part_events = Vec::new();
        sim.advance_with(TimeMs::new(900.0).unwrap(), &mut |event| {
            on_part_events.push(*event)
        })
        .unwrap();
        assert!(sim.active_sliders.is_empty());
        assert!(!sim.sliders[0].as_ref().unwrap().tracking());

        sim.advance_with(TimeMs::new(1000.0).unwrap(), &mut |event| {
            on_part_events.push(*event)
        })
        .unwrap();
        assert_eq!(sim.active_sliders, [ObjectId(0)]);
        assert!(sim.sliders[0].as_ref().unwrap().tracking());

        sim.advance_with(TimeMs::new(2500.0).unwrap(), &mut |event| {
            on_part_events.push(*event)
        })
        .unwrap();
        assert!(sim.active_sliders.is_empty());
        assert!(!sim.sliders[0].as_ref().unwrap().tracking());
        assert_eq!(
            on_part_events,
            [
                head_event(900.0, 0, 0),
                tick_event(1500.0, 0, 0, false, 0, (1, 0)),
                tick_event(2000.0, 0, 1, true, 10, (0, 1)),
                tail_event(2464.0, 0, 2, true, 30, (1, 2)),
                result_event(
                    2500.0,
                    0,
                    Judgement::Ok,
                    100 + 4 * u64::from(map.difficulty_multiplier()),
                    2
                ),
            ]
        );
        assert_eq!(
            sim.stats().score,
            170 + 4 * u64::from(map.difficulty_multiplier())
        );
    }

    #[test]
    fn eof_with_equal_time_records_releases_tracking_and_generates_no_extra_click() {
        // EOF между частями в группе с несколькими записями в одно время сбрасывает tracking сразу.
        let slider_only = slider_map(ONE_TICK_SLIDER);
        let early_eof = replay(&[(1000, 0.0, 1), (250, 50.0, 0), (0, 50.0, 1)]);
        let mut sim = Simulator::new(&slider_only, &early_eof).unwrap();
        let mut events = Vec::new();
        sim.advance_with(TimeMs::new(1250.0).unwrap(), &mut |event| {
            events.push(*event)
        })
        .unwrap();
        assert_eq!(sim.active_sliders, [ObjectId(0)]);
        assert!(!sim.sliders[0].as_ref().unwrap().tracking());

        sim.advance_with(TimeMs::new(2000.0).unwrap(), &mut |event| {
            events.push(*event)
        })
        .unwrap();
        assert!(sim.active_sliders.is_empty());
        assert!(!sim.sliders[0].as_ref().unwrap().tracking());
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                tick_event(1500.0, 0, 0, false, 0, (1, 0)),
                tail_event(1964.0, 0, 1, false, 0, (0, 0)),
                result_event(2000.0, 0, Judgement::Meh, 50, 0),
            ]
        );
        assert_eq!(sim.stats().score, 80);

        // EOF в момент тика после двух нажатий в одной группе: круги поражаются,
        // лишний клик не создаётся, а тик и хвост пропускаются из-за отпускания EOF.
        let mixed = slider_map(concat!(
            "0,0,1000,2,0,L|200:0,1,200\n",
            "100,0,1500,1,0\n",
            "100,0,1500,1,0\n",
            "100,0,1500,1,0",
        ));
        let eof_on_tick = replay(&[
            (1000, 0.0, 1),
            (500, 100.0, 0),
            (0, 100.0, 1),
            (0, 100.0, 3),
        ]);
        let (stats, events) = run_with_journal(&mixed, &eof_on_tick, [2000.0]);
        let second_circle_score = 300 + 12 * u64::from(mixed.difficulty_multiplier());
        assert_eq!(
            events,
            [
                head_event(1000.0, 0, 0),
                SimEvent {
                    time: TimeMs::new(1500.0).unwrap(),
                    object_id: ObjectId(1),
                    kind: EventKind::Circle,
                    part_index: None,
                    part_hit: None,
                    judgement: Some(Judgement::Great),
                    score_delta: 300,
                    combo_before: 1,
                    combo_after: 2,
                },
                SimEvent {
                    time: TimeMs::new(1500.0).unwrap(),
                    object_id: ObjectId(2),
                    kind: EventKind::Circle,
                    part_index: None,
                    part_hit: None,
                    judgement: Some(Judgement::Great),
                    score_delta: second_circle_score,
                    combo_before: 2,
                    combo_after: 3,
                },
                tick_event(1500.0, 0, 0, false, 0, (3, 0)),
                SimEvent {
                    time: TimeMs::new(1620.0).unwrap(),
                    object_id: ObjectId(3),
                    kind: EventKind::Circle,
                    part_index: None,
                    part_hit: None,
                    judgement: Some(Judgement::Miss),
                    score_delta: 0,
                    combo_before: 0,
                    combo_after: 0,
                },
                tail_event(1964.0, 0, 1, false, 0, (0, 0)),
                result_event(2000.0, 0, Judgement::Meh, 50, 0),
            ]
        );
        assert_eq!(stats.score, 380 + second_circle_score);
        assert_eq!(stats.max_combo, 3);
        assert_eq!(stats.current_combo, 0);
        assert_eq!(stats.counts.count_300, 2);
        assert_eq!(stats.counts.count_miss, 1);
    }
}
