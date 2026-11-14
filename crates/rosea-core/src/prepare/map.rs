//! Общий вход подготовки карты.

use crate::error::RoseaError;
use crate::model::{Beatmap, ObjectId, TimeMs};

use super::{
    SliderEventKind, StackedObject, StackedObjectKind, Visibility, stack_objects, visibility,
};

const MAX_TIME: f64 = 86_400_000.0;
const MAX_SCHEDULE_EVENTS: usize = 2_300_000;

/// Виды событий, подготовленных для временной шкалы карты.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapEventKind {
    SpinnerStart,
    SliderStart,
    SliderTick,
    SliderRepeat,
    SliderTailCheck,
    CircleDeadline,
    SliderHeadDeadline,
    SliderEnd,
    SpinnerEnd,
}

/// Событие карты; индекс части слайдера ссылается на его подготовленную шкалу.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MapEvent {
    pub time: TimeMs,
    pub object_id: ObjectId,
    pub kind: MapEventKind,
    pub part_index: Option<u32>,
}

impl MapEvent {
    fn order_key(self) -> (u8, u32, u32, u32) {
        let id = self.object_id.0;
        let part = self.part_index.unwrap_or(0);
        match self.kind {
            MapEventKind::SpinnerStart => (1, id, 0, 0),
            MapEventKind::SliderStart => (3, 0, id, part),
            MapEventKind::SliderTick => (3, 1, id, part),
            MapEventKind::SliderRepeat => (3, 2, id, part),
            MapEventKind::SliderTailCheck => (3, 3, id, part),
            MapEventKind::CircleDeadline | MapEventKind::SliderHeadDeadline => (4, id, 0, 0),
            MapEventKind::SliderEnd | MapEventKind::SpinnerEnd => (4, id, 1, 0),
        }
    }
}

/// Полуокна попаданий в миллисекундах.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HitWindows {
    pub great: i64,
    pub ok: i64,
    pub meh: i64,
}

/// Подготовленные времена одного объекта в порядке объектов карты.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ObjectTimes {
    pub object_id: ObjectId,
    pub start: TimeMs,
    pub end: TimeMs,
    pub head_deadline: Option<TimeMs>,
    /// Предельное время решения при отсутствии успешного нажатия головы.
    pub latest_resolution: TimeMs,
}

/// Подготовленные объекты и параметры их видимости.
#[derive(Debug)]
pub struct PreparedMap {
    objects: Vec<StackedObject>,
    visibility: Visibility,
    hit_windows: HitWindows,
    object_times: Vec<ObjectTimes>,
    events: Vec<MapEvent>,
}

impl PreparedMap {
    pub fn objects(&self) -> &[StackedObject] {
        &self.objects
    }

    pub fn visibility(&self) -> &Visibility {
        &self.visibility
    }

    pub fn first_visible_at(&self) -> TimeMs {
        self.visibility.first_visible_at
    }

    pub fn hit_windows(&self) -> HitWindows {
        self.hit_windows
    }

    pub fn object_times(&self) -> &[ObjectTimes] {
        &self.object_times
    }

    pub fn events(&self) -> &[MapEvent] {
        &self.events
    }
}

/// Строит пути, времена слайдеров и stacking для разобранной карты.
pub fn prepare_map(map: &Beatmap) -> Result<PreparedMap, RoseaError> {
    let objects = stack_objects(
        map.objects(),
        map.difficulty(),
        map.timing_points(),
        map.stack_leniency(),
    )?;
    let od = f64::from(map.difficulty().od);
    let hit_windows = HitWindows {
        great: (80.0 - 6.0 * od).floor() as i64,
        ok: (140.0 - 8.0 * od).floor() as i64,
        meh: (200.0 - 10.0 * od).floor() as i64,
    };
    let mut object_times = Vec::with_capacity(objects.len());
    for object in &objects {
        let checked = |value: f64| -> Result<TimeMs, RoseaError> {
            let time = TimeMs::new(value).map_err(|_| RoseaError::NumericalError {
                line: object.source_line(),
                object_id: object.object_id(),
            })?;
            if value.abs() > MAX_TIME {
                return Err(RoseaError::ResourceLimit {
                    resource: "absolute_time_ms",
                    limit: MAX_TIME as u64,
                    observed: value.abs().ceil() as u64,
                });
            }
            Ok(time)
        };
        let start = checked(object.start_time())?;
        let (end, head_deadline) = match object.kind() {
            StackedObjectKind::Circle => {
                (start, Some(checked(start.get() + hit_windows.meh as f64)?))
            }
            StackedObjectKind::Slider(_) => {
                let slider = object.slider().ok_or(RoseaError::PreprocessError {
                    line: object.source_line(),
                    object_id: object.object_id(),
                    message: "Не найден подготовленный слайдер",
                })?;
                (
                    checked(slider.timing().end_time)?,
                    Some(checked(start.get() + hit_windows.meh as f64)?),
                )
            }
            StackedObjectKind::Spinner { end } => (checked(end)?, None),
        };
        let latest_resolution = head_deadline
            .filter(|deadline| deadline.get() > end.get())
            .unwrap_or(end);
        object_times.push(ObjectTimes {
            object_id: object.object_id(),
            start,
            end,
            head_deadline,
            latest_resolution,
        });
    }
    let mut event_count = 0_usize;
    for object in &objects {
        let count = match object.kind() {
            StackedObjectKind::Circle => 1,
            StackedObjectKind::Spinner { .. } => 2,
            StackedObjectKind::Slider(_) => {
                3 + object.slider().map_or(0, |s| s.timing().events.len())
            }
        };
        event_count = event_count
            .checked_add(count)
            .ok_or(RoseaError::ResourceLimit {
                resource: "map_events",
                limit: MAX_SCHEDULE_EVENTS as u64,
                observed: u64::MAX,
            })?;
        if event_count > MAX_SCHEDULE_EVENTS {
            return Err(RoseaError::ResourceLimit {
                resource: "map_events",
                limit: MAX_SCHEDULE_EVENTS as u64,
                observed: event_count as u64,
            });
        }
    }
    let mut events = Vec::with_capacity(event_count);
    for (object, times) in objects.iter().zip(&object_times) {
        let mut add = |time, kind, part_index| {
            events.push(MapEvent {
                time,
                object_id: object.object_id(),
                kind,
                part_index,
            });
        };
        match object.kind() {
            StackedObjectKind::Circle => {
                if let Some(deadline) = times.head_deadline {
                    add(deadline, MapEventKind::CircleDeadline, None);
                }
            }
            StackedObjectKind::Slider(_) => {
                add(times.start, MapEventKind::SliderStart, None);
                if let Some(slider) = object.slider() {
                    for (index, part) in slider.timing().events.iter().enumerate() {
                        let time =
                            TimeMs::new(part.time).map_err(|_| RoseaError::NumericalError {
                                line: object.source_line(),
                                object_id: object.object_id(),
                            })?;
                        let kind = match part.kind {
                            SliderEventKind::Tick => MapEventKind::SliderTick,
                            SliderEventKind::Repeat => MapEventKind::SliderRepeat,
                            SliderEventKind::TailCheck => MapEventKind::SliderTailCheck,
                        };
                        add(time, kind, Some(index as u32));
                    }
                }
                if let Some(deadline) = times.head_deadline {
                    add(deadline, MapEventKind::SliderHeadDeadline, None);
                }
                add(times.end, MapEventKind::SliderEnd, None);
            }
            StackedObjectKind::Spinner { .. } => {
                add(times.start, MapEventKind::SpinnerStart, None);
                add(times.end, MapEventKind::SpinnerEnd, None);
            }
        }
    }
    events.sort_by(|a, b| {
        a.time
            .get()
            .total_cmp(&b.time.get())
            .then_with(|| a.order_key().cmp(&b.order_key()))
    });
    Ok(PreparedMap {
        objects,
        visibility: visibility(map)?,
        hit_windows,
        object_times,
        events,
    })
}
