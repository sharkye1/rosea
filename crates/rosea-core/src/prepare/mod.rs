//! Геометрия и временная шкала L/B-слайдеров для симуляции и рисования.

mod curve;
mod timing;

pub use curve::{PreparedPath, linear_path, slider_path};
pub use timing::{
    PreparedSlider, SliderEvent, SliderEventKind, SliderTiming, linear_sliders, sliders,
};
