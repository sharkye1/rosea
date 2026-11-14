//! Общий вход подготовки карты.

use crate::error::RoseaError;
use crate::model::{Beatmap, ObjectId, TimeMs};

use super::{StackedObject, StackedObjectKind, Visibility, stack_objects, visibility};

const MAX_TIME: f64 = 86_400_000.0;

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
    Ok(PreparedMap {
        objects,
        visibility: visibility(map)?,
        hit_windows,
        object_times,
    })
}
