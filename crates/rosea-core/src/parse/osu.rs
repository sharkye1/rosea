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

pub fn osu_mode(bytes: &[u8]) -> Result<u32, RoseaError> {
    osu_header(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    let mut in_general = false;
    let mut mode = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') {
            in_general = line == "[General]";
            continue;
        }
        if !in_general {
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            if line == "Mode" {
                return Err(RoseaError::InvalidMode);
            }
            continue;
        };
        if key.trim() != "Mode" {
            continue;
        }
        if mode.is_some() {
            return Err(RoseaError::DuplicateField);
        }
        let value: u32 = value.trim().parse().map_err(|_| RoseaError::InvalidMode)?;
        if value != 0 {
            return Err(RoseaError::UnsupportedMode(value));
        }
        mode = Some(value);
    }

    Ok(mode.unwrap_or(0))
}
