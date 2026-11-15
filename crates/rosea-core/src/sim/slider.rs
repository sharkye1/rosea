//! Проверка удержания шарика слайдера.

use crate::error::RoseaError;
use crate::model::{ObjectId, Point, Stats, TimeMs};
use crate::prepare::{HitWindows, PreparedSlider, StackedObject};
use crate::replay::ButtonState;

use super::hit::{inside_radius, judge_click};
use super::score::apply_slider_head;

/// Состояние головы и удержания одного подготовленного слайдера.
pub struct SliderState<'a> {
    object: &'a StackedObject,
    slider: &'a PreparedSlider,
    head_hit: bool,
    tracking: bool,
}

impl<'a> SliderState<'a> {
    pub fn new(object: &'a StackedObject) -> Option<Self> {
        Some(Self {
            object,
            slider: object.slider()?,
            head_hit: false,
            tracking: false,
        })
    }

    pub fn object_id(&self) -> ObjectId {
        self.object.object_id()
    }

    pub fn head_hit(&self) -> bool {
        self.head_hit
    }

    pub fn tracking(&self) -> bool {
        self.tracking
    }

    /// Обрабатывает один фронт нажатия по ещё не поражённой голове.
    pub fn hit_head(
        &mut self,
        click_ms: i64,
        cursor: Point,
        buttons: ButtonState,
        windows: HitWindows,
        radius: f32,
        stats: &mut Stats,
    ) -> Result<bool, RoseaError> {
        if self.head_hit
            || !inside_radius(cursor, self.object.position(), radius)
            || judge_click(windows, TimeMs::new(self.object.start_time())?, click_ms).is_none()
        {
            return Ok(false);
        }
        let time = click_ms as f64;
        let tracking =
            if time >= self.slider.timing().start_time && time <= self.slider.timing().end_time {
                let ball = self.object.position_at_time(time)?;
                tracking_at_sample(true, buttons, cursor, ball, radius)
            } else {
                false
            };
        apply_slider_head(stats)?;
        self.head_hit = true;
        self.tracking = tracking;
        Ok(true)
    }

    /// Обновляет удержание на заданной выборке, используя подготовленный путь.
    pub fn sample(
        &mut self,
        time: TimeMs,
        cursor: Point,
        buttons: ButtonState,
        radius: f32,
    ) -> Result<bool, RoseaError> {
        let time = time.get();
        self.tracking =
            if time < self.slider.timing().start_time || time > self.slider.timing().end_time {
                false
            } else {
                let ball = self.object.position_at_time(time)?;
                tracking_at_sample(self.tracking, buttons, cursor, ball, radius)
            };
        Ok(self.tracking)
    }
}

/// Возвращает tracking после одной выборки курсора и кнопок.
pub fn tracking_at_sample(
    tracking: bool,
    buttons: ButtonState,
    cursor: Point,
    ball: Point,
    radius: f32,
) -> bool {
    if !buttons.left && !buttons.right {
        return false;
    }
    let threshold = if tracking { radius * 2.4_f32 } else { radius };
    inside_radius(cursor, ball, threshold)
}
