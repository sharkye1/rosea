//! Проверка соответствия карты и реплея.
#![forbid(unsafe_code)]

use rosea_core::{error::RoseaError, model::ReplayMeta, parse::beatmap_md5};

mod hash;

pub fn check_beatmap_hash(bytes: &[u8], replay: &ReplayMeta) -> Result<(), RoseaError> {
    hash::check_hash(beatmap_md5(bytes), replay)
}
