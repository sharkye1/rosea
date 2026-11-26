//! Загрузка и headless-прогон пары карты и реплея.
#![forbid(unsafe_code)]

use rosea_core::{error::RoseaError, model::ReplayMeta, parse::beatmap_md5};

mod hash;
mod load;
mod run;

pub use load::{AppError, LoadedPair, ReadError, load_pair};
pub use run::{HeadlessRun, run_headless};

pub fn check_beatmap_hash(bytes: &[u8], replay: &ReplayMeta) -> Result<(), RoseaError> {
    hash::check_hash(beatmap_md5(bytes), replay)
}
