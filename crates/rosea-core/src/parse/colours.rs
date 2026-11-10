//! Чтение секции `[Colours]`, цветов комбо и переопределений слайдера.

use crate::error::RoseaError;
use crate::model::{Colours, DEFAULT_COMBO_COLOURS, Rgb};

use super::osu_mode;

const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Читает цветовую информацию карты из секции `[Colours]`.
///
/// Поддерживает ключи Combo1..Combo8, SliderTrackOverride и SliderBorder.
/// Повторное объявление любого поддерживаемого ключа возвращает ошибку `DuplicateField`.
/// Неизвестные поля и комментарии пропускаются. При отсутствии секции или ключей Combo
/// используются 4 стандартных цвета комбо.
pub fn osu_colours(bytes: &[u8]) -> Result<Colours, RoseaError> {
    osu_mode(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;

    let mut in_colours = false;
    let mut combo_slots: [Option<Rgb>; 8] = [None; 8];
    let mut slider_track_override = None;
    let mut slider_border = None;

    for (index, raw_line) in text.lines().enumerate() {
        if raw_line.len() > MAX_LINE_BYTES {
            return Err(RoseaError::ResourceLimit {
                resource: "osu_line_bytes",
                limit: MAX_LINE_BYTES as u64,
                observed: raw_line.len() as u64,
            });
        }
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') {
            in_colours = line == "[Colours]";
            continue;
        }
        if !in_colours {
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            let key = line.trim();
            if is_supported_colour_key(key) {
                return Err(RoseaError::InvalidColourField {
                    field: supported_field_name(key),
                    line: index + 1,
                });
            }
            continue;
        };

        let key = key.trim();
        let value = value.trim();

        if let Some(n) = parse_combo_index(key) {
            let slot = &mut combo_slots[n - 1];
            if slot.is_some() {
                return Err(RoseaError::DuplicateField);
            }
            let field = supported_field_name(key);
            *slot = Some(parse_rgb(value, field, index + 1)?);
            continue;
        }

        match key {
            "SliderTrackOverride" => {
                if slider_track_override.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                slider_track_override = Some(parse_rgb(value, "SliderTrackOverride", index + 1)?);
            }
            "SliderBorder" => {
                if slider_border.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                slider_border = Some(parse_rgb(value, "SliderBorder", index + 1)?);
            }
            _ => continue,
        }
    }

    let has_any_custom_combo = combo_slots.iter().any(Option::is_some);
    let combo = if has_any_custom_combo {
        combo_slots.into_iter().flatten().collect()
    } else {
        DEFAULT_COMBO_COLOURS.to_vec()
    };

    Ok(Colours {
        combo,
        slider_track_override,
        slider_border,
    })
}

fn parse_combo_index(key: &str) -> Option<usize> {
    let suffix = key.strip_prefix("Combo")?;
    let n: usize = suffix.parse().ok()?;
    if (1..=8).contains(&n) { Some(n) } else { None }
}

fn is_supported_colour_key(key: &str) -> bool {
    parse_combo_index(key).is_some() || matches!(key, "SliderTrackOverride" | "SliderBorder")
}

fn supported_field_name(key: &str) -> &'static str {
    match key {
        "Combo1" => "Combo1",
        "Combo2" => "Combo2",
        "Combo3" => "Combo3",
        "Combo4" => "Combo4",
        "Combo5" => "Combo5",
        "Combo6" => "Combo6",
        "Combo7" => "Combo7",
        "Combo8" => "Combo8",
        "SliderTrackOverride" => "SliderTrackOverride",
        "SliderBorder" => "SliderBorder",
        _ => "unknown",
    }
}

fn parse_rgb(value: &str, field: &'static str, line: usize) -> Result<Rgb, RoseaError> {
    let invalid = || RoseaError::InvalidColourField { field, line };
    let mut parts = value.split(',');
    let r_str = parts.next().ok_or_else(invalid)?;
    let g_str = parts.next().ok_or_else(invalid)?;
    let b_str = parts.next().ok_or_else(invalid)?;
    if parts.next().is_some() {
        return Err(invalid());
    }

    let parse_channel = |s: &str| -> Result<u8, RoseaError> {
        let trimmed = s.trim();
        let val: u32 = trimmed.parse().map_err(|_| invalid())?;
        if val > 255 {
            return Err(invalid());
        }
        Ok(val as u8)
    };

    Ok(Rgb::new(
        parse_channel(r_str)?,
        parse_channel(g_str)?,
        parse_channel(b_str)?,
    ))
}
