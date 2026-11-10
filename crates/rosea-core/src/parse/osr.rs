use crate::error::RoseaError;

// Разобранные данные пока используются только внутренними тестами.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
struct ReplayHeader<'a> {
    mode: u8,
    game_version: i32,
    beatmap_hash: &'a str,
    player: &'a str,
    replay_hash: &'a str,
    // Порядок файла: 300, 100, 50, geki, katu, miss.
    counts: [u16; 6],
    score: i32,
    max_combo: u16,
    perfect: u8,
    mods: u32,
    life_graph: &'a str,
    timestamp: [u8; 8],
    compressed_data: &'a [u8],
    trailer: &'a [u8],
    warnings: Vec<ReplayWarning>,
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
enum ReplayWarning {
    ReplayTrailerNotInterpreted,
}

#[allow(dead_code)]
fn read_header(bytes: &[u8]) -> Result<ReplayHeader<'_>, RoseaError> {
    let mut cursor = Cursor::new(bytes);
    let mode = cursor.read_u8()?;
    let game_version = cursor.read_i32()?;
    let beatmap_hash = cursor.read_string()?;
    let player = cursor.read_string()?;
    let replay_hash = cursor.read_string()?;
    let mut counts = [0; 6];
    for count in &mut counts {
        *count = cursor.read_u16()?;
    }
    let score = cursor.read_i32()?;
    let max_combo = cursor.read_u16()?;
    let perfect = cursor.read_u8()?;
    let mods_offset = cursor.offset;
    let mods = cursor.read_u32()?;
    let life_graph = cursor.read_string()?;
    let timestamp = cursor.read_bytes()?;
    let length_offset = cursor.offset;
    let length = cursor.read_i32()?;
    if length == -1 || length == 0 {
        return Err(RoseaError::NoReplayData {
            offset: length_offset,
        });
    }
    let length = usize::try_from(length).map_err(|_| RoseaError::InvalidLength {
        offset: length_offset,
    })?;
    let end = cursor
        .offset
        .checked_add(length)
        .ok_or(RoseaError::InvalidLength {
            offset: length_offset,
        })?;
    let compressed_data = bytes
        .get(cursor.offset..end)
        .ok_or(RoseaError::UnexpectedEof {
            offset: cursor.offset,
        })?;
    let trailer = &bytes[end..];
    if mode != 0 {
        return Err(RoseaError::UnsupportedReplayMode { mode, offset: 0 });
    }
    if mods != 0 {
        return Err(RoseaError::UnsupportedMods {
            mods,
            offset: mods_offset,
        });
    }
    let warnings = if trailer.is_empty() {
        Vec::new()
    } else {
        vec![ReplayWarning::ReplayTrailerNotInterpreted]
    };
    Ok(ReplayHeader {
        mode,
        game_version,
        beatmap_hash,
        player,
        replay_hash,
        counts,
        score,
        max_combo,
        perfect,
        mods,
        life_graph,
        timestamp,
        compressed_data,
        trailer,
        warnings,
    })
}

#[allow(dead_code)]
struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[allow(dead_code)]
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_bytes<const N: usize>(&mut self) -> Result<[u8; N], RoseaError> {
        let error = || RoseaError::UnexpectedEof {
            offset: self.offset,
        };
        let end = self.offset.checked_add(N).ok_or_else(error)?;
        let bytes = self.bytes.get(self.offset..end).ok_or_else(error)?;
        let mut result = [0; N];
        result.copy_from_slice(bytes);
        self.offset = end;
        Ok(result)
    }

    fn read_u8(&mut self) -> Result<u8, RoseaError> {
        Ok(u8::from_le_bytes(self.read_bytes()?))
    }

    fn read_u16(&mut self) -> Result<u16, RoseaError> {
        Ok(u16::from_le_bytes(self.read_bytes()?))
    }

    fn read_i32(&mut self) -> Result<i32, RoseaError> {
        Ok(i32::from_le_bytes(self.read_bytes()?))
    }

    fn read_u32(&mut self) -> Result<u32, RoseaError> {
        Ok(u32::from_le_bytes(self.read_bytes()?))
    }

    fn read_uleb128(&mut self) -> Result<usize, RoseaError> {
        let start = self.offset;
        let mut value = 0u64;
        for shift in (0..=63).step_by(7) {
            let byte = self.read_u8()?;
            let payload = u64::from(byte & 0x7f);
            if shift == 63 && (payload > 1 || byte & 0x80 != 0) {
                return Err(RoseaError::InvalidLength { offset: start });
            }
            value |= payload << shift;
            if byte & 0x80 == 0 {
                return usize::try_from(value)
                    .map_err(|_| RoseaError::InvalidLength { offset: start });
            }
        }
        Err(RoseaError::InvalidLength { offset: start })
    }

    fn read_string(&mut self) -> Result<&'a str, RoseaError> {
        let start = self.offset;
        match self.read_u8()? {
            0 => Ok(""),
            0x0b => {
                let length = self.read_uleb128()?;
                const MAX_STRING_BYTES: usize = 1024 * 1024;
                if length > MAX_STRING_BYTES {
                    return Err(RoseaError::ResourceLimit {
                        resource: "binary string bytes",
                        limit: MAX_STRING_BYTES as u64,
                        observed: length as u64,
                    });
                }
                let start = self.offset;
                let end = start
                    .checked_add(length)
                    .ok_or(RoseaError::InvalidLength { offset: start })?;
                let bytes = self
                    .bytes
                    .get(start..end)
                    .ok_or(RoseaError::UnexpectedEof { offset: start })?;
                let text = std::str::from_utf8(bytes)
                    .map_err(|_| RoseaError::InvalidReplayString { offset: start })?;
                self.offset = end;
                Ok(text)
            }
            _ => Err(RoseaError::InvalidStringMarker { offset: start }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_replay() -> Vec<u8> {
        let mut bytes = vec![0];
        bytes.extend_from_slice(&20131216_i32.to_le_bytes());
        bytes.extend_from_slice(&[0x0b, 1, b'b', 0x0b, 1, b'p', 0x0b, 1, b'r']);
        for count in 1_u16..=6 {
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        bytes.extend_from_slice(&(-42_i32).to_le_bytes());
        bytes.extend_from_slice(&7_u16.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&[0x0b, 1, b'l']);
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        bytes.extend_from_slice(&3_i32.to_le_bytes());
        bytes.extend_from_slice(&[10, 11, 12]);
        bytes
    }

    #[test]
    fn reads_header_and_separates_trailer() {
        for version in [0_i32, 1, 20131216, i32::MAX] {
            for trailer in [&[][..], &[1, 2, 3, 4][..], &[5; 8][..], &[6; 13][..]] {
                let mut bytes = example_replay();
                bytes[1..5].copy_from_slice(&version.to_le_bytes());
                bytes.extend_from_slice(trailer);
                let header = read_header(&bytes).unwrap();
                assert_eq!(header.game_version, version);
                assert_eq!(
                    (header.beatmap_hash, header.player, header.replay_hash),
                    ("b", "p", "r")
                );
                assert_eq!(header.counts, [1, 2, 3, 4, 5, 6]);
                assert_eq!(
                    (header.score, header.max_combo, header.perfect, header.mods),
                    (-42, 7, 1, 0)
                );
                assert_eq!(header.life_graph, "l");
                assert_eq!(header.timestamp, [1, 2, 3, 4, 5, 6, 7, 8]);
                assert_eq!(header.compressed_data, &[10, 11, 12]);
                assert_eq!(header.trailer, trailer);
                let expected_warnings = if trailer.is_empty() {
                    vec![]
                } else {
                    vec![ReplayWarning::ReplayTrailerNotInterpreted]
                };
                assert_eq!(header.warnings, expected_warnings);
            }
        }
    }

    #[test]
    fn rejects_truncation_modes_mods_and_invalid_lengths() {
        let bytes = example_replay();
        for end in 0..bytes.len() {
            assert!(matches!(
                read_header(&bytes[..end]),
                Err(RoseaError::UnexpectedEof { .. })
            ));
        }
        let mut changed = bytes.clone();
        changed[0] = 1;
        assert_eq!(
            read_header(&changed),
            Err(RoseaError::UnsupportedReplayMode { mode: 1, offset: 0 })
        );
        let mut changed = bytes.clone();
        let mods_offset = 5 + 9 + 12 + 4 + 2 + 1;
        changed[mods_offset..mods_offset + 4].copy_from_slice(&8_u32.to_le_bytes());
        assert_eq!(
            read_header(&changed),
            Err(RoseaError::UnsupportedMods {
                mods: 8,
                offset: mods_offset
            })
        );
        let length_offset = bytes.len() - 7;
        for length in [-1_i32, 0, -2] {
            let mut changed = bytes.clone();
            changed[length_offset..length_offset + 4].copy_from_slice(&length.to_le_bytes());
            let expected = if length >= -1 {
                RoseaError::NoReplayData {
                    offset: length_offset,
                }
            } else {
                RoseaError::InvalidLength {
                    offset: length_offset,
                }
            };
            assert_eq!(read_header(&changed), Err(expected));
        }
    }

    #[test]
    fn reads_numbers_in_order() {
        let mut cursor = Cursor::new(&[
            0xab, 0x34, 0x12, 0xfe, 0xff, 0xff, 0xff, 0x78, 0x56, 0x34, 0x12,
        ]);
        assert_eq!(cursor.read_u8(), Ok(0xab));
        assert_eq!(cursor.read_u16(), Ok(0x1234));
        assert_eq!(cursor.read_i32(), Ok(-2));
        assert_eq!(cursor.read_u32(), Ok(0x12345678));
        assert_eq!(
            cursor.read_u8(),
            Err(RoseaError::UnexpectedEof { offset: 11 })
        );
    }

    #[test]
    fn truncated_number_keeps_position() {
        for len in 0..4 {
            let bytes = [0x42, 1, 2, 3, 4];
            let mut cursor = Cursor::new(&bytes[..len + 1]);
            assert_eq!(cursor.read_u8(), Ok(0x42));
            assert_eq!(
                cursor.read_u32(),
                Err(RoseaError::UnexpectedEof { offset: 1 })
            );
            assert_eq!(cursor.offset, 1);
        }
    }

    #[test]
    fn reads_uleb128_and_rejects_overflow_and_truncation() {
        for (bytes, expected) in [
            (&[0][..], 0),
            (&[0x7f][..], 127),
            (&[0x80, 1][..], 128),
            (&[0xe5, 0x8e, 0x26][..], 624485),
        ] {
            assert_eq!(Cursor::new(bytes).read_uleb128(), Ok(expected));
        }
        assert_eq!(
            Cursor::new(&[0x80]).read_uleb128(),
            Err(RoseaError::UnexpectedEof { offset: 1 })
        );
        assert_eq!(
            Cursor::new(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2]).read_uleb128(),
            Err(RoseaError::InvalidLength { offset: 0 })
        );
        assert_eq!(
            Cursor::new(&[0x80; 10]).read_uleb128(),
            Err(RoseaError::InvalidLength { offset: 0 })
        );
    }

    #[test]
    fn reads_strings_and_reports_bad_input() {
        let mut cursor = Cursor::new(&[0, 0x0b, 0, 0x0b, 4, 0xd1, 0x82, 0xd0, 0xb5]);
        assert_eq!(cursor.read_string(), Ok(""));
        assert_eq!(cursor.read_string(), Ok(""));
        assert_eq!(cursor.read_string(), Ok("те"));
        assert_eq!(
            Cursor::new(&[1]).read_string(),
            Err(RoseaError::InvalidStringMarker { offset: 0 })
        );
        assert_eq!(
            Cursor::new(&[0x0b, 2, 0xff, 0xff]).read_string(),
            Err(RoseaError::InvalidReplayString { offset: 2 })
        );
        assert_eq!(
            Cursor::new(&[0x0b, 2, 0x61]).read_string(),
            Err(RoseaError::UnexpectedEof { offset: 2 })
        );
        assert_eq!(
            Cursor::new(&[0x0b, 0x80]).read_string(),
            Err(RoseaError::UnexpectedEof { offset: 2 })
        );
    }

    #[test]
    fn enforces_binary_string_limit() {
        let mut bytes = vec![0x0b, 0x80, 0x80, 0x40];
        bytes.resize(4 + 1024 * 1024, b'a');
        assert_eq!(
            Cursor::new(&bytes).read_string().map(str::len),
            Ok(1024 * 1024)
        );
        assert_eq!(
            Cursor::new(&[0x0b, 0x81, 0x80, 0x40]).read_string(),
            Err(RoseaError::ResourceLimit {
                resource: "binary string bytes",
                limit: 1024 * 1024,
                observed: 1024 * 1024 + 1,
            })
        );
    }
}
