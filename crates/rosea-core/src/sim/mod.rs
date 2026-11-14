//! Игровые правила симуляции.

pub mod hit;
pub mod input;
pub mod schedule;
pub mod score;
mod simulator;
pub mod timeline;

pub use simulator::{EventKind, SimEvent, Simulator};
