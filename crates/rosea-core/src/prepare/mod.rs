//! Геометрия и временная шкала линейных слайдеров для симуляции и рисования.

mod curve;
mod timing;

pub use curve::{PreparedPath, linear_path};
pub use timing::{PreparedSlider, SliderEvent, SliderEventKind, SliderTiming, linear_sliders};
