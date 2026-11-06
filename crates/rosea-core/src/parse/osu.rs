use crate::error::RoseaError;

#[derive(Debug, PartialEq)]
pub struct Difficulty {
    pub cs: f64,
    pub od: f64,
    pub hp: f64,
}

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

/// Читает только CS, OD и HP; остальные параметры сложности пока пропускает.
pub fn osu_difficulty(bytes: &[u8]) -> Result<Difficulty, RoseaError> {
    osu_header(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    let mut in_difficulty = false;
    let mut cs = None;
    let mut od = None;
    let mut hp = None;

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') {
            in_difficulty = line == "[Difficulty]";
            continue;
        }
        if !in_difficulty {
            continue;
        }

        let (key, value) = line.split_once(':').unwrap_or((line, ""));
        let (field, slot) = match key.trim() {
            "CircleSize" => ("CircleSize", &mut cs),
            "OverallDifficulty" => ("OverallDifficulty", &mut od),
            "HPDrainRate" => ("HPDrainRate", &mut hp),
            _ => continue,
        };
        if slot.is_some() {
            return Err(RoseaError::DuplicateField);
        }
        let invalid = || RoseaError::InvalidDifficultyField {
            field,
            line: index + 1,
        };
        let value: f64 = value.trim().parse().map_err(|_| invalid())?;
        if !value.is_finite() || !(0.0..=10.0).contains(&value) {
            return Err(invalid());
        }
        *slot = Some(value);
    }

    Ok(Difficulty {
        cs: cs.ok_or(RoseaError::MissingDifficultyField("CircleSize"))?,
        od: od.ok_or(RoseaError::MissingDifficultyField("OverallDifficulty"))?,
        hp: hp.ok_or(RoseaError::MissingDifficultyField("HPDrainRate"))?,
    })
}
