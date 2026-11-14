//! Общий вход подготовки карты.

use crate::error::RoseaError;
use crate::model::{Beatmap, TimeMs};

use super::{StackedObject, Visibility, stack_objects, visibility};

/// Подготовленные объекты и параметры их видимости.
#[derive(Debug)]
pub struct PreparedMap {
    objects: Vec<StackedObject>,
    visibility: Visibility,
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
}

/// Строит пути, времена слайдеров и stacking для разобранной карты.
pub fn prepare_map(map: &Beatmap) -> Result<PreparedMap, RoseaError> {
    let objects = stack_objects(
        map.objects(),
        map.difficulty(),
        map.timing_points(),
        map.stack_leniency(),
    )?;
    Ok(PreparedMap {
        objects,
        visibility: visibility(map)?,
    })
}
