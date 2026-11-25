use rosea_core::{error::RoseaError, model::ReplayMeta};

pub(crate) fn check_hash(hash: [u8; 16], replay: &ReplayMeta) -> Result<(), RoseaError> {
    let expected = replay.beatmap_hash().as_bytes();
    if expected.len() != 32 {
        return Err(RoseaError::BeatmapHashMismatch);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (byte, pair) in hash.iter().zip(expected.chunks_exact(2)) {
        if !pair[0].eq_ignore_ascii_case(&HEX[(byte >> 4) as usize])
            || !pair[1].eq_ignore_ascii_case(&HEX[(byte & 15) as usize])
        {
            return Err(RoseaError::BeatmapHashMismatch);
        }
    }
    Ok(())
}
