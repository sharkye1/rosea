use crate::error::RoseaError;
use crate::model::{
    ExpectedStats, Parsed, ParsedReplay, Point, ReplayInput, ReplayMeta, ReplayRecord, Warning,
};
use std::io::{self, Write};

const LZMA_DICTIONARY_LIMIT: usize = 64 * 1024 * 1024;
const REPLAY_OUTPUT_LIMIT: usize = 256 * 1024 * 1024;

fn read_records(bytes: &[u8]) -> Result<ReplayInput, RoseaError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        let index = bytes[..error.valid_up_to()]
            .iter()
            .filter(|&&b| b == b',')
            .count();
        RoseaError::InvalidReplayRecord {
            index: index as u32,
        }
    })?;
    if text.is_empty() {
        return Err(RoseaError::NoReplayData { offset: 0 });
    }
    let text = text.strip_suffix(',').unwrap_or(text);
    let mut records = Vec::new();
    let mut seed = None;
    for (index, record) in text.split(',').enumerate() {
        let index = u32::try_from(index).map_err(|_| RoseaError::ResourceLimit {
            resource: "replay records",
            limit: 1_000_000,
            observed: index as u64,
        })?;
        let invalid = || RoseaError::InvalidReplayRecord { index };
        if seed.is_some() {
            return Err(invalid());
        }
        let mut fields = record.split('|');
        let delta = fields.next().ok_or_else(invalid)?;
        let x = fields.next().ok_or_else(invalid)?;
        let y = fields.next().ok_or_else(invalid)?;
        let keys = fields.next().ok_or_else(invalid)?;
        if fields.next().is_some() {
            return Err(invalid());
        }
        let delta_ms = match delta.parse::<i64>() {
            Ok(value) => value,
            Err(_) => {
                if delta
                    .parse::<f64>()
                    .is_ok_and(|value| value.is_finite() && value.fract() != 0.0)
                {
                    return Err(RoseaError::UnsupportedReplayFeature { index });
                }
                return Err(invalid());
            }
        };
        let x = x.parse::<f32>().map_err(|_| invalid())?;
        let y = y.parse::<f32>().map_err(|_| invalid())?;
        if !x.is_finite() || !y.is_finite() {
            return Err(invalid());
        }
        if delta_ms == -12345 && x == 0.0 && y == 0.0 {
            seed = Some(keys.parse::<i32>().map_err(|_| invalid())?);
        } else {
            let keys = keys.parse::<u32>().map_err(|_| invalid())?;
            if records.len() == 1_000_000 {
                return Err(RoseaError::ResourceLimit {
                    resource: "replay records",
                    limit: 1_000_000,
                    observed: 1_000_001,
                });
            }
            records.push(ReplayRecord {
                index,
                delta_ms,
                cursor: Point { x, y },
                keys,
            });
        }
    }
    if records.is_empty() {
        return Err(RoseaError::NoReplayData { offset: 0 });
    }
    Ok(ReplayInput { records, seed })
}

pub fn osr(bytes: &[u8]) -> Result<Parsed<ParsedReplay>, RoseaError> {
    let header = read_header(bytes)?;
    let offset = bytes.len() - header.trailer.len() - header.compressed_data.len();
    let data = decompress_replay(header.compressed_data, offset)?;
    let input = read_records(&data)?;
    let expected = ExpectedStats {
        score: header.score,
        max_combo: header.max_combo,
        count_300: header.counts[0],
        count_100: header.counts[1],
        count_50: header.counts[2],
        count_miss: header.counts[5],
    };
    let meta = ReplayMeta {
        mode: header.mode,
        game_version: header.game_version,
        beatmap_hash: header.beatmap_hash.to_owned(),
        player: header.player.to_owned(),
        replay_hash: header.replay_hash.to_owned(),
        count_geki: header.counts[3],
        count_katu: header.counts[4],
        perfect: header.perfect,
        mods: header.mods,
        life_graph: header.life_graph.to_owned(),
        timestamp: header.timestamp,
        trailer: header.trailer.to_vec(),
    };
    let warnings = if header.trailer.is_empty() {
        Vec::new()
    } else {
        vec![Warning::ReplayTrailerNotInterpreted]
    };
    Ok(Parsed {
        value: ParsedReplay {
            meta,
            expected,
            input,
        },
        warnings,
    })
}

struct LimitedOutput {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: Option<u64>,
}

impl Write for LimitedOutput {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let len = self.bytes.len().saturating_add(data.len());
        if len > self.limit {
            self.exceeded = Some(len as u64);
            return Err(io::Error::other("Превышен лимит распакованных данных"));
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn decompress_replay(data: &[u8], offset: usize) -> Result<Vec<u8>, RoseaError> {
    let header = data.get(..13).ok_or(RoseaError::InvalidLzma { offset })?;
    let dictionary = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as u64;
    if dictionary > LZMA_DICTIONARY_LIMIT as u64 {
        return Err(RoseaError::ResourceLimit {
            resource: "LZMA dictionary",
            limit: LZMA_DICTIONARY_LIMIT as u64,
            observed: dictionary,
        });
    }
    let size = u64::from_le_bytes([
        header[5], header[6], header[7], header[8], header[9], header[10], header[11], header[12],
    ]);
    if size != u64::MAX && size > REPLAY_OUTPUT_LIMIT as u64 {
        return Err(RoseaError::ResourceLimit {
            resource: "decompressed replay",
            limit: REPLAY_OUTPUT_LIMIT as u64,
            observed: size,
        });
    }
    let mut output = LimitedOutput {
        bytes: Vec::new(),
        limit: REPLAY_OUTPUT_LIMIT,
        exceeded: None,
    };
    let options = lzma_rs::decompress::Options {
        unpacked_size: lzma_rs::decompress::UnpackedSize::ReadFromHeader,
        memlimit: Some(LZMA_DICTIONARY_LIMIT),
        allow_incomplete: false,
    };
    let result = lzma_rs::lzma_decompress_with_options(&mut &data[..], &mut output, &options);
    if let Some(observed) = output.exceeded {
        return Err(RoseaError::ResourceLimit {
            resource: "decompressed replay",
            limit: REPLAY_OUTPUT_LIMIT as u64,
            observed,
        });
    }
    result.map_err(|_| RoseaError::InvalidLzma { offset })?;
    if output.bytes.is_empty() {
        return Err(RoseaError::NoReplayData { offset });
    }
    Ok(output.bytes)
}

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
}

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
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

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

    #[test]
    fn reads_replay_records_and_seed() {
        let input = read_records(b"10|256|192|5,5|257|192|0,-12345|0|0|-42,").unwrap();
        assert_eq!(input.seed, Some(-42));
        assert_eq!(
            input.records,
            vec![
                ReplayRecord {
                    index: 0,
                    delta_ms: 10,
                    cursor: Point { x: 256.0, y: 192.0 },
                    keys: 5
                },
                ReplayRecord {
                    index: 1,
                    delta_ms: 5,
                    cursor: Point { x: 257.0, y: 192.0 },
                    keys: 0
                },
            ]
        );
        let input = read_records(b"-10|-5|500|4294967295").unwrap();
        assert_eq!(input.records[0].delta_ms, -10);
        assert_eq!(input.records[0].cursor, Point { x: -5.0, y: 500.0 });
        assert_eq!(input.seed, None);
    }

    #[test]
    fn rejects_bad_records_and_seed_positions() {
        for text in [
            "1|0|0|0,,2|0|0|0",
            "1|0|0|0,,",
            "1|0|0",
            "1|NaN|0|0",
            "1|0|inf|0",
            "1|0|0|-1",
            "1|0|0|0|1",
            "-12345|0|0|1,1|0|0|0",
            "1|0|0|0,-12345|0|0|1,-12345|0|0|2",
        ] {
            assert!(
                matches!(
                    read_records(text.as_bytes()),
                    Err(RoseaError::InvalidReplayRecord { .. })
                ),
                "{text}"
            );
        }
        assert_eq!(
            read_records(b"1.5|0|0|0"),
            Err(RoseaError::UnsupportedReplayFeature { index: 0 })
        );
        assert_eq!(
            read_records(b"1|0|0|0,\xff"),
            Err(RoseaError::InvalidReplayRecord { index: 1 })
        );
        assert!(matches!(
            read_records(b""),
            Err(RoseaError::NoReplayData { .. })
        ));
        assert!(matches!(
            read_records(b"-12345|0|0|1"),
            Err(RoseaError::NoReplayData { .. })
        ));
    }

    #[test]
    fn enforces_record_count_limit() {
        let mut data = "0|0|0|0,".repeat(1_000_000);
        assert_eq!(
            read_records(data.as_bytes()).unwrap().records.len(),
            1_000_000
        );
        data.push_str("0|0|0|0");
        assert!(matches!(
            read_records(data.as_bytes()),
            Err(RoseaError::ResourceLimit {
                resource: "replay records",
                observed: 1_000_001,
                ..
            })
        ));
    }

    #[test]
    fn decompresses_and_checks_lzma_limits() {
        let data = [
            93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 12, 3, 65, 4,
            153, 91, 83, 160, 122, 219, 176, 37, 27, 45, 1, 127, 255, 225, 190, 128, 0,
        ];
        assert_eq!(decompress_replay(&data, 50).unwrap(), b"10|256|192|5,");
        let mut replay = example_replay();
        let length_offset = replay.len() - 7;
        replay.truncate(length_offset);
        replay.extend_from_slice(&(data.len() as i32).to_le_bytes());
        replay.extend_from_slice(&data);
        replay.extend_from_slice(&[7, 8]);
        let parsed = osr(&replay).unwrap();
        assert_eq!(parsed.value.meta.trailer, &[7, 8]);
        assert_eq!(parsed.warnings, vec![Warning::ReplayTrailerNotInterpreted]);
        assert_eq!(parsed.value.input.records[0].delta_ms, 10);
        assert_eq!(
            parsed.value.input.records[0].cursor,
            Point { x: 256.0, y: 192.0 }
        );
        assert_eq!(parsed.value.input.records[0].keys, 5);
        assert_eq!(
            decompress_replay(&data[..12], 50),
            Err(RoseaError::InvalidLzma { offset: 50 })
        );
        assert_eq!(
            decompress_replay(&data[..16], 50),
            Err(RoseaError::InvalidLzma { offset: 50 })
        );
        let mut bad = data;
        bad[1..5].copy_from_slice(&(LZMA_DICTIONARY_LIMIT as u32 + 1).to_le_bytes());
        assert!(matches!(
            decompress_replay(&bad, 50),
            Err(RoseaError::ResourceLimit {
                resource: "LZMA dictionary",
                ..
            })
        ));
        let mut bad = data;
        bad[5..13].copy_from_slice(&(REPLAY_OUTPUT_LIMIT as u64 + 1).to_le_bytes());
        assert!(matches!(
            decompress_replay(&bad, 50),
            Err(RoseaError::ResourceLimit {
                resource: "decompressed replay",
                ..
            })
        ));
        let mut output = LimitedOutput {
            bytes: Vec::new(),
            limit: 2,
            exceeded: None,
        };
        assert_eq!(output.write(b"ab").unwrap(), 2);
        assert!(output.write(b"c").is_err());
        assert_eq!(output.exceeded, Some(3));
        assert_eq!(output.bytes, b"ab");
    }

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
