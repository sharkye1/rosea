//! Объединение событий карты и групп записей реплея по времени.

use crate::error::RoseaError;
use crate::model::TimeMs;
use crate::prepare::{MapEvent, PreparedMap};
use crate::replay::{ReplayTimeline, TimedRecord};

/// Записи реплея и события карты с одним временем.
#[derive(Debug)]
pub struct TimeGroup<'a> {
    pub time: TimeMs,
    pub records: &'a [TimedRecord],
    pub map_events: &'a [MapEvent],
    pub last_input: bool,
}

/// Проходит оба источника, не копируя записи и события.
pub struct Schedule<'a> {
    map: &'a PreparedMap,
    replay: &'a ReplayTimeline,
    map_index: usize,
    group_index: usize,
    last_time: Option<TimeMs>,
}

impl<'a> Schedule<'a> {
    pub fn new(map: &'a PreparedMap, replay: &'a ReplayTimeline) -> Self {
        Self {
            map,
            replay,
            map_index: 0,
            group_index: 0,
            last_time: None,
        }
    }

    /// Передаёт каждую полную группу до заданного времени включительно.
    pub fn advance_to(
        &mut self,
        time: TimeMs,
        mut emit: impl FnMut(TimeGroup<'a>),
    ) -> Result<(), RoseaError> {
        if self.last_time.is_some_and(|last| time.get() < last.get()) {
            return Err(RoseaError::TimeWentBackwards);
        }
        while self
            .next_time()?
            .is_some_and(|next| next.get() <= time.get())
        {
            if let Some(group) = self.next_group()? {
                emit(group);
            }
        }
        self.last_time = Some(time);
        Ok(())
    }

    fn next_time(&self) -> Result<Option<TimeMs>, RoseaError> {
        let events = self.map.events();
        let records = self.replay.records();
        let groups = self.replay.groups();
        let next_map = events.get(self.map_index).map(|event| event.time);
        let next_input = groups
            .get(self.group_index)
            .map(|group| records[group.start].time_ms as f64);
        match (next_map, next_input) {
            (None, None) => Ok(None),
            (Some(time), None) => Ok(Some(time)),
            (Some(time), Some(input)) if time.get() < input => Ok(Some(time)),
            (_, Some(input)) => Ok(Some(TimeMs::new(input)?)),
        }
    }

    fn next_group(&mut self) -> Result<Option<TimeGroup<'a>>, RoseaError> {
        let Some(time) = self.next_time()? else {
            return Ok(None);
        };
        let events = self.map.events();
        let records = self.replay.records();
        let groups = self.replay.groups();
        let next_input = groups
            .get(self.group_index)
            .map(|group| records[group.start].time_ms as f64);
        let start = self.map_index;
        while self.map_index < events.len() && events[self.map_index].time.get() == time.get() {
            self.map_index += 1;
        }
        let input = if next_input == Some(time.get()) {
            let group = &groups[self.group_index];
            self.group_index += 1;
            &records[group.clone()]
        } else {
            &records[0..0]
        };
        Ok(Some(TimeGroup {
            time,
            records: input,
            map_events: &events[start..self.map_index],
            last_input: !input.is_empty() && self.group_index == groups.len(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, ReplayInput, ReplayRecord};
    use crate::parse::osu;
    use crate::prepare::prepare_map;
    use crate::replay::normalize;

    fn fixture() -> (PreparedMap, ReplayTimeline) {
        let map = b"osu file format v14\n[Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\n[HitObjects]\n0,0,-110,1,0\n100,0,-99.75,1,0\n200,0,-80,1,0";
        let map = prepare_map(&osu(map).unwrap().value).unwrap();
        let input = ReplayInput {
            records: [0, 10, 0, 20, 20]
                .into_iter()
                .enumerate()
                .map(|(index, delta_ms)| ReplayRecord {
                    index: index as u32,
                    delta_ms,
                    cursor: Point {
                        x: index as f32,
                        y: 0.0,
                    },
                    keys: index as u32,
                })
                .collect(),
            seed: None,
        };
        (map, normalize(input).unwrap().value)
    }

    #[test]
    fn merges_sources_and_advances_whole_groups_once() {
        let (map, replay) = fixture();
        let mut schedule = Schedule::new(&map, &replay);
        let mut groups = Vec::new();
        let mut capture = |group: TimeGroup<'_>| {
            groups.push((
                group.time.get(),
                group.records.iter().map(|r| r.index).collect::<Vec<_>>(),
                group
                    .map_events
                    .iter()
                    .map(|event| event.object_id.0)
                    .collect::<Vec<_>>(),
                group.last_input,
            ))
        };
        schedule
            .advance_to(TimeMs::new(-1.0).unwrap(), &mut capture)
            .unwrap();
        schedule
            .advance_to(TimeMs::new(0.0).unwrap(), &mut capture)
            .unwrap();
        schedule
            .advance_to(TimeMs::new(10.0).unwrap(), &mut capture)
            .unwrap();
        schedule
            .advance_to(TimeMs::new(10.0).unwrap(), &mut capture)
            .unwrap();
        assert_eq!(
            schedule.advance_to(TimeMs::new(9.0).unwrap(), &mut capture),
            Err(RoseaError::TimeWentBackwards)
        );
        schedule
            .advance_to(TimeMs::new(100.0).unwrap(), &mut capture)
            .unwrap();
        assert_eq!(
            groups,
            vec![
                (0.0, vec![0], vec![], false),
                (10.0, vec![1, 2], vec![0], false),
                (20.25, vec![], vec![1], false),
                (30.0, vec![3], vec![], false),
                (40.0, vec![], vec![2], false),
                (50.0, vec![4], vec![], true),
            ]
        );
    }

    #[test]
    fn first_request_may_be_after_all_events() {
        let (map, replay) = fixture();
        let mut schedule = Schedule::new(&map, &replay);
        let mut count = 0;
        schedule
            .advance_to(TimeMs::new(100.0).unwrap(), |_| count += 1)
            .unwrap();
        schedule
            .advance_to(TimeMs::new(200.0).unwrap(), |_| count += 1)
            .unwrap();
        assert_eq!(count, 6);
    }
}
