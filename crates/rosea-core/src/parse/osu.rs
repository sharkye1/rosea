use crate::error::RoseaError;

pub fn osu_header(bytes: &[u8]) -> Result<u32, RoseaError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let version = line
            .strip_prefix("osu file format v")
            .ok_or(RoseaError::InvalidHeader)?;
        let version: u32 = version.parse().map_err(|_| RoseaError::InvalidHeader)?;
        if version != 14 {
            return Err(RoseaError::UnsupportedVersion(version));
        }
        return Ok(version);
    }

    Err(RoseaError::InvalidHeader)
}
