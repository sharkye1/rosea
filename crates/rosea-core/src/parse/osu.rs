//! Заголовок .osu и поддерживаемые поля секций General и Difficulty.

use crate::error::RoseaError;
use crate::model::{Beatmap, ObjectId, Parsed, Warning};

const MAX_LINE_BYTES: usize = 1024 * 1024;

pub fn osu(bytes: &[u8]) -> Result<Parsed<Beatmap>, RoseaError> {
    let text = osu_text(bytes)?;
    for (index, line) in text.lines().enumerate() {
        check_line_length(line, index + 1)?;
    }
    super::osu_mode(bytes)?;
    let value = Beatmap {
        difficulty: osu_difficulty(bytes)?,
        stack_leniency: osu_stack_leniency(bytes)?,
        objects: super::osu_hit_objects(bytes)?,
        timing_points: super::osu_timing_points(bytes)?,
        breaks: super::osu_breaks(bytes)?,
        metadata: super::osu_metadata(bytes)?,
        colours: super::osu_colours(bytes)?,
    };
    let mut section = "";
    let mut count = 0_u32;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') {
            section = line;
            if !matches!(
                section,
                "[General]"
                    | "[Metadata]"
                    | "[Difficulty]"
                    | "[TimingPoints]"
                    | "[Colours]"
                    | "[HitObjects]"
                    | "[Events]"
            ) {
                count = count.saturating_add(1);
            }
            continue;
        }
        let key = line.split_once(':').map_or(line, |(key, _)| key).trim();
        let known = match section {
            "[General]" => matches!(key, "Mode" | "StackLeniency"),
            "[Metadata]" => matches!(
                key,
                "Title"
                    | "TitleUnicode"
                    | "Artist"
                    | "ArtistUnicode"
                    | "Creator"
                    | "Version"
                    | "Source"
                    | "Tags"
                    | "BeatmapID"
                    | "BeatmapSetID"
            ),
            "[Difficulty]" => matches!(
                key,
                "CircleSize"
                    | "OverallDifficulty"
                    | "HPDrainRate"
                    | "ApproachRate"
                    | "SliderMultiplier"
                    | "SliderTickRate"
            ),
            "[Colours]" => matches!(
                key,
                "Combo1"
                    | "Combo2"
                    | "Combo3"
                    | "Combo4"
                    | "Combo5"
                    | "Combo6"
                    | "Combo7"
                    | "Combo8"
                    | "SliderTrackOverride"
                    | "SliderBorder"
            ),
            _ => true,
        };
        if !known {
            count = count.saturating_add(1);
        }
    }
    let warnings = if count == 0 {
        vec![]
    } else {
        vec![Warning::UnknownBeatmapFields { count }]
    };
    Ok(Parsed { value, warnings })
}

#[derive(Debug, PartialEq)]
pub struct Difficulty {
    pub cs: f32,
    pub od: f32,
    pub hp: f32,
    pub ar: f32,
    // Без слайдеров отсутствие этих полей допустимо, поэтому сохраняем Option.
    pub slider_multiplier: Option<f64>,
    pub slider_tick_rate: Option<f64>,
}

fn check_line_length(line: &str, line_number: usize) -> Result<(), RoseaError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(RoseaError::MapResourceLimit {
            resource: "osu_line_bytes",
            limit: MAX_LINE_BYTES as u64,
            observed: (line.len() as u64).into(),
            line: line_number,
            object_id: None,
        });
    }
    Ok(())
}

pub(super) fn osu_text(bytes: &[u8]) -> Result<&str, RoseaError> {
    std::str::from_utf8(bytes).map_err(|error| RoseaError::InvalidEncoding {
        line: bytes[..error.valid_up_to()]
            .split(|&byte| byte == b'\n')
            .count(),
    })
}

pub fn osu_header(bytes: &[u8]) -> Result<u32, RoseaError> {
    let text = osu_text(bytes)?;
    // BOM допустим только в начале. Убираем его из текстового представления.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Комментарии до заголовка не допускаются: первая непустая строка задаёт версию.
        let line_number = index + 1;
        let invalid = || RoseaError::InvalidHeader {
            line: Some(line_number),
        };
        let version = line.strip_prefix("osu file format v").ok_or_else(invalid)?;
        let version: u32 = version.parse().map_err(|_| invalid())?;
        if version != 14 {
            return Err(RoseaError::UnsupportedBeatmapVersion {
                version,
                line: line_number,
            });
        }
        return Ok(version);
    }

    Err(RoseaError::InvalidHeader { line: None })
}

pub fn osu_mode(bytes: &[u8]) -> Result<u32, RoseaError> {
    osu_header(bytes)?;
    let text = osu_text(bytes)?;
    let mut in_general = false;
    // Не сбрасываем при повторной секции General: второй Mode остаётся дубликатом.
    let mut mode = None;

    for (index, line) in text.lines().enumerate() {
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

        let line_number = index + 1;
        let Some((key, value)) = line.split_once(':') else {
            if line == "Mode" {
                return Err(RoseaError::InvalidMode { line: line_number });
            }
            continue;
        };
        if key.trim() != "Mode" {
            continue;
        }
        if mode.is_some() {
            return Err(RoseaError::DuplicateField {
                field: "Mode",
                line: line_number,
            });
        }
        let value: u32 = value
            .trim()
            .parse()
            .map_err(|_| RoseaError::InvalidMode { line: line_number })?;
        if value != 0 {
            return Err(RoseaError::UnsupportedMode {
                mode: value,
                line: line_number,
            });
        }
        mode = Some(value);
    }

    // Отсутствующий Mode означает Standard по правилам формата.
    Ok(mode.unwrap_or(0))
}

/// Читает StackLeniency из General; без поля или секции возвращает 0.7.
pub fn osu_stack_leniency(bytes: &[u8]) -> Result<f32, RoseaError> {
    osu_mode(bytes)?;
    let text = osu_text(bytes)?;
    let mut in_general = false;
    let mut leniency = None;

    for (index, raw_line) in text.lines().enumerate() {
        check_line_length(raw_line, index + 1)?;
        let line = raw_line.trim();
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
        let (key, value) = line.split_once(':').unwrap_or((line, ""));
        if key.trim() != "StackLeniency" {
            continue;
        }
        // Повторная секция General не разрешает второе объявление того же поля.
        if leniency.is_some() {
            return Err(RoseaError::DuplicateField {
                field: "StackLeniency",
                line: index + 1,
            });
        }
        let invalid = || RoseaError::InvalidStackLeniency { line: index + 1 };
        let value: f32 = value.trim().parse().map_err(|_| invalid())?;
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(invalid());
        }
        leniency = Some(value);
    }

    Ok(leniency.unwrap_or(0.7))
}

/// Читает сложность; при отсутствии AR использует OD.
/// SliderMultiplier и SliderTickRate обязательны, если в HitObjects есть слайдер.
pub fn osu_difficulty(bytes: &[u8]) -> Result<Difficulty, RoseaError> {
    osu_header(bytes)?;
    let text = osu_text(bytes)?;
    let mut in_difficulty = false;
    let mut in_objects = false;
    let mut object_index = 0_u32;
    let mut has_sliders = false;
    let mut cs = None;
    let mut od = None;
    let mut hp = None;
    let mut ar = None;
    let mut slider_multiplier = None;
    let mut slider_tick_rate = None;

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') {
            in_difficulty = line == "[Difficulty]";
            in_objects = line == "[HitObjects]";
            continue;
        }
        if in_objects {
            if object_index == 100_000 {
                return Err(RoseaError::MapResourceLimit {
                    resource: "hit_objects",
                    limit: 100_000,
                    observed: (u64::from(object_index) + 1).into(),
                    line: index + 1,
                    object_id: Some(ObjectId(object_index)),
                });
            }
            // Для обязательности параметров читаем только флаг типа.
            // Геометрия и остальные поля остаются задачей парсера объектов.
            let flags: u8 = line
                .split(',')
                .nth(3)
                .unwrap_or("")
                .trim()
                .parse()
                .map_err(|_| RoseaError::InvalidHitObjectField {
                    field: "type",
                    line: index + 1,
                    object_id: ObjectId(object_index),
                })?;
            has_sliders |= flags & 2 != 0;
            object_index += 1;
            continue;
        }
        if !in_difficulty {
            continue;
        }

        // У поддерживаемого ключа без двоеточия пустое значение вызовет ошибку числа.
        let (key, value) = line.split_once(':').unwrap_or((line, ""));
        let line_number = index + 1;
        match key.trim() {
            "CircleSize" => {
                read_difficulty_value(&mut cs, value, "CircleSize", line_number, false)?
            }
            "OverallDifficulty" => {
                read_difficulty_value(&mut od, value, "OverallDifficulty", line_number, false)?
            }
            "HPDrainRate" => {
                read_difficulty_value(&mut hp, value, "HPDrainRate", line_number, false)?
            }
            "ApproachRate" => {
                read_difficulty_value(&mut ar, value, "ApproachRate", line_number, false)?
            }
            "SliderMultiplier" => read_difficulty_value(
                &mut slider_multiplier,
                value,
                "SliderMultiplier",
                line_number,
                true,
            )?,
            "SliderTickRate" => read_difficulty_value(
                &mut slider_tick_rate,
                value,
                "SliderTickRate",
                line_number,
                true,
            )?,
            _ => continue,
        }
    }

    // Проверяем наличие после всей секции: порядок полей в файле произвольный.
    // Подстановка AR выполняется, когда итоговый OD уже известен.
    let cs = cs.ok_or(RoseaError::MissingDifficultyField("CircleSize"))?;
    let od = od.ok_or(RoseaError::MissingDifficultyField("OverallDifficulty"))?;
    let hp = hp.ok_or(RoseaError::MissingDifficultyField("HPDrainRate"))?;
    // Проверяем после всего файла: HitObjects может идти до или после Difficulty.
    if has_sliders {
        slider_multiplier.ok_or(RoseaError::MissingDifficultyField("SliderMultiplier"))?;
        slider_tick_rate.ok_or(RoseaError::MissingDifficultyField("SliderTickRate"))?;
    }
    Ok(Difficulty {
        cs,
        od,
        hp,
        ar: ar.unwrap_or(od),
        slider_multiplier,
        slider_tick_rate,
    })
}

fn read_difficulty_value<T: std::str::FromStr>(
    slot: &mut Option<T>,
    text: &str,
    field: &'static str,
    line: usize,
    slider_setting: bool,
) -> Result<(), RoseaError> {
    if slot.is_some() {
        return Err(RoseaError::DuplicateField { field, line });
    }
    let invalid = || RoseaError::InvalidDifficultyField { field, line };
    let text = text.trim();
    let raw: f64 = text.parse().map_err(|_| invalid())?;
    let in_range = if slider_setting {
        raw > 0.0
    } else {
        (0.0..=10.0).contains(&raw)
    };
    if !raw.is_finite() || !in_range {
        return Err(invalid());
    }
    *slot = Some(text.parse().map_err(|_| invalid())?);
    Ok(())
}
