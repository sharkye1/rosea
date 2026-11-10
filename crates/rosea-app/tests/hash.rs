use rosea_app::check_beatmap_hash;
use rosea_core::{error::RoseaError, parse::osr};

fn replay(hash: &str) -> Vec<u8> {
    let data = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 12, 3, 65, 4, 153,
        91, 83, 160, 122, 219, 176, 37, 27, 45, 1, 127, 255, 225, 190, 128, 0,
    ];
    let mut bytes = vec![0];
    bytes.extend_from_slice(&1_i32.to_le_bytes());
    bytes.extend_from_slice(&[0x0b, hash.len() as u8]);
    bytes.extend_from_slice(hash.as_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&[0; 12 + 4 + 2 + 1 + 4 + 1 + 8]);
    bytes.extend_from_slice(&(data.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes
}

#[test]
fn compares_raw_map_hash_case_insensitively() {
    let map = b"\xef\xbb\xbfosu file format v14\r\n";
    let bytes = replay("F8C6CB69306064D2C6E0FB8DD41547D8");
    let parsed = osr(&bytes).unwrap();
    assert_eq!(check_beatmap_hash(map, &parsed.value.meta), Ok(()));
    assert_eq!(
        check_beatmap_hash(b"osu file format v14\r\n", &parsed.value.meta),
        Err(RoseaError::BeatmapHashMismatch)
    );
    assert_eq!(
        check_beatmap_hash(b"\xef\xbb\xbfosu file format v14\n", &parsed.value.meta),
        Err(RoseaError::BeatmapHashMismatch)
    );
    let bad = replay("not-hex");
    assert_eq!(
        check_beatmap_hash(map, &osr(&bad).unwrap().value.meta),
        Err(RoseaError::BeatmapHashMismatch)
    );
}
