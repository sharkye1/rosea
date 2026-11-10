use crate::error::RoseaError;

// Курсор будет подключён к разбору реплея при реализации заголовка.
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
