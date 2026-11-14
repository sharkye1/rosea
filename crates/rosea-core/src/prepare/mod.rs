//! Геометрия, временная шкала слайдеров и stacking для симуляции и рисования.

mod curve;
mod stacking;
mod timing;
mod visibility;

pub use curve::{PreparedPath, linear_path, slider_path};
pub use stacking::{StackedObject, stack_objects};
pub use timing::{
    PreparedSlider, SliderEvent, SliderEventKind, SliderTiming, linear_sliders, sliders,
};
pub use visibility::{Visibility, visibility};
