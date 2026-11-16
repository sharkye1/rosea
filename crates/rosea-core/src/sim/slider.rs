//! Проверка удержания шарика слайдера.

use crate::error::RoseaError;
use crate::model::{ObjectId, Point, Stats, TimeMs};
use crate::prepare::{
    HitWindows, MapEvent, MapEventKind, PreparedSlider, SliderEventKind, StackedObject,
};
use crate::replay::ButtonState;

use super::hit::{inside_radius, judge_click};
use super::input::InputEvent;
use super::score::{apply_slider_head, apply_slider_repeat, apply_slider_tail, apply_slider_tick};

/// Состояние головы и удержания одного подготовленного слайдера.
pub struct SliderState<'a> {
    object: &'a StackedObject,
    slider: &'a PreparedSlider,
    head_result: Option<bool>,
    tracking: bool,
    last_input_sample_time: Option<TimeMs>,
    last_map_sample_time: Option<TimeMs>,
    processed_parts: Vec<bool>,
    successful_parts: u32,
}

impl<'a> SliderState<'a> {
    pub fn new(object: &'a StackedObject) -> Option<Self> {
        let slider = object.slider()?;
        Some(Self {
            object,
            slider,
            head_result: None,
            tracking: false,
            last_input_sample_time: None,
            last_map_sample_time: None,
            processed_parts: vec![false; slider.timing().events.len()],
            successful_parts: 0,
        })
    }

    pub fn object_id(&self) -> ObjectId {
        self.object.object_id()
    }

    pub fn head_hit(&self) -> bool {
        self.head_result == Some(true)
    }

    pub fn tracking(&self) -> bool {
        self.tracking
    }

    pub fn successful_parts(&self) -> u32 {
        self.successful_parts
    }

    pub fn total_parts(&self) -> u32 {
        self.processed_parts.len() as u32 + 1
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
        if self.head_result.is_some()
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
        self.head_result = Some(true);
        self.successful_parts += 1;
        self.tracking = tracking;
        Ok(true)
    }

    /// Закрывает нерешённую голову после обработки ввода в момент срока.
    pub fn timeout_head(&mut self, time: TimeMs, windows: HitWindows, stats: &mut Stats) -> bool {
        if self.head_result.is_some()
            || time.get() < self.slider.timing().start_time + windows.meh as f64
        {
            return false;
        }
        self.head_result = Some(false);
        stats.current_combo = 0;
        true
    }

    /// Применяет тик после выборки tracking; повторный индекс игнорируется.
    pub fn tick(&mut self, index: u32, stats: &mut Stats) -> Result<Option<bool>, RoseaError> {
        let index = index as usize;
        let Some(event) = self.slider.timing().events.get(index) else {
            return Ok(None);
        };
        if event.kind != SliderEventKind::Tick || self.processed_parts[index] {
            return Ok(None);
        }
        let hit = self.tracking;
        apply_slider_tick(stats, hit)?;
        self.processed_parts[index] = true;
        if hit {
            self.successful_parts += 1;
        }
        Ok(Some(hit))
    }

    /// Применяет повтор после выборки tracking; повторный индекс игнорируется.
    pub fn repeat(&mut self, index: u32, stats: &mut Stats) -> Result<Option<bool>, RoseaError> {
        let index = index as usize;
        let Some(event) = self.slider.timing().events.get(index) else {
            return Ok(None);
        };
        if event.kind != SliderEventKind::Repeat || self.processed_parts[index] {
            return Ok(None);
        }
        let hit = self.tracking;
        apply_slider_repeat(stats, hit)?;
        self.processed_parts[index] = true;
        if hit {
            self.successful_parts += 1;
        }
        Ok(Some(hit))
    }

    /// Применяет единственную проверку хвоста после выборки tracking.
    pub fn tail_check(
        &mut self,
        index: u32,
        stats: &mut Stats,
    ) -> Result<Option<bool>, RoseaError> {
        let index = index as usize;
        let Some(event) = self.slider.timing().events.get(index) else {
            return Ok(None);
        };
        if event.kind != SliderEventKind::TailCheck || self.processed_parts[index] {
            return Ok(None);
        }
        let hit = self.tracking;
        apply_slider_tail(stats, hit)?;
        self.processed_parts[index] = true;
        if hit {
            self.successful_parts += 1;
        }
        Ok(Some(hit))
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

    /// Обновляет tracking после завершённой записи ввода или отпускания EOF.
    pub fn observe_input_event(
        &mut self,
        time: TimeMs,
        event: InputEvent,
        radius: f32,
    ) -> Result<Option<bool>, RoseaError> {
        match event {
            InputEvent::RecordEnd {
                cursor, buttons, ..
            } => {
                let tracking = self.sample(time, cursor, buttons, radius)?;
                self.last_input_sample_time = Some(time);
                Ok(Some(tracking))
            }
            InputEvent::End { cursor } => {
                let tracking = self.sample(time, cursor, ButtonState::default(), radius)?;
                self.last_input_sample_time = Some(time);
                Ok(Some(tracking))
            }
            InputEvent::Record { .. } | InputEvent::Press { .. } => Ok(None),
        }
    }

    /// Делает одну выборку для группы, если она содержит событие этого слайдера.
    pub fn sample_for_map_events(
        &mut self,
        time: TimeMs,
        cursor: Point,
        buttons: ButtonState,
        radius: f32,
        events: &[MapEvent],
    ) -> Result<Option<bool>, RoseaError> {
        let has_slider_event = events.iter().any(|event| {
            event.object_id == self.object_id()
                && event.time == time
                && matches!(
                    event.kind,
                    MapEventKind::SliderStart
                        | MapEventKind::SliderTick
                        | MapEventKind::SliderRepeat
                        | MapEventKind::SliderTailCheck
                )
        });
        if !has_slider_event {
            return Ok(None);
        }
        if self.last_input_sample_time == Some(time) {
            return Ok(Some(self.tracking));
        }
        if self.last_map_sample_time == Some(time) {
            return Ok(Some(self.tracking));
        }
        let tracking = self.sample(time, cursor, buttons, radius)?;
        self.last_map_sample_time = Some(time);
        Ok(Some(tracking))
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
