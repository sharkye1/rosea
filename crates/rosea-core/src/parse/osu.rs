//! Заголовок .osu и поддерживаемые поля секций General и Difficulty.

use crate::error::RoseaError;
use crate::model::ObjectId;

#[derive(Debug, PartialEq)]
pub struct Difficulty {
    pub cs: f64,
    pub od: f64,
    pub hp: f64,
    pub ar: f64,
    // Без слайдеров отсутствие этих полей допустимо, поэтому сохраняем Option.
    pub slider_multiplier: Option<f64>,
    pub slider_tick_rate: Option<f64>,
}

pub fn osu_header(bytes: &[u8]) -> Result<u32, RoseaError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    // BOM допустим только в начале. Убираем его из текстового представления.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Комментарии до заголовка не допускаются: первая непустая строка задаёт версию.
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
    // Не сбрасываем при повторной секции General: второй Mode остаётся дубликатом.
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

    // Отсутствующий Mode означает Standard по правилам формата.
    Ok(mode.unwrap_or(0))
}

/// Читает сложность; при отсутствии AR использует OD.
/// SliderMultiplier и SliderTickRate обязательны, если в HitObjects есть слайдер.
pub fn osu_difficulty(bytes: &[u8]) -> Result<Difficulty, RoseaError> {
    osu_header(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
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
                return Err(RoseaError::ResourceLimit {
                    resource: "hit_objects",
                    limit: 100_000,
                    observed: u64::from(object_index) + 1,
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
        let (field, slot) = match key.trim() {
            "CircleSize" => ("CircleSize", &mut cs),
            "OverallDifficulty" => ("OverallDifficulty", &mut od),
            "HPDrainRate" => ("HPDrainRate", &mut hp),
            "ApproachRate" => ("ApproachRate", &mut ar),
            "SliderMultiplier" => ("SliderMultiplier", &mut slider_multiplier),
            "SliderTickRate" => ("SliderTickRate", &mut slider_tick_rate),
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
        // Параметры слайдеров имеют собственный диапазон: любое конечное число > 0.
        let is_slider_setting = matches!(field, "SliderMultiplier" | "SliderTickRate");
        let in_range = if is_slider_setting {
            value > 0.0
        } else {
            (0.0..=10.0).contains(&value)
        };
        if !value.is_finite() || !in_range {
            return Err(invalid());
        }
        *slot = Some(value);
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
