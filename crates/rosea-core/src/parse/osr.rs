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
}
