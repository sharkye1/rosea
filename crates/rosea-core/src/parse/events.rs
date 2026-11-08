//! Чтение секции `[Events]` и интервалов перерывов.

use crate::error::RoseaError;
use crate::model::BreakPeriod;

use super::osu_mode;

const MAX_BREAKS: usize = 100_000;
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Читает интервалы перерывов из секции `[Events]`.
///
/// Остальные типы событий (фон, видео, спрайты, сэмплы, анимации, команды storyboard)
/// пропускаются без ошибки, включая строки с запятыми внутри имён файлов.
/// Отрицательная длительность перерыва нормализуется до нуля (`end = max(start, end)`).
/// Результат упорядочивается по времени начала.
pub fn osu_breaks(bytes: &[u8]) -> Result<Vec<BreakPeriod>, RoseaError> {
    osu_mode(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    let mut in_events = false;
    let mut breaks = Vec::new();

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
            in_events = line == "[Events]";
            continue;
        }
        if !in_events {
            continue;
        }

        // Пропускаем команды storyboard, начинающиеся с символа подчёркивания.
        if line.starts_with('_') {
            continue;
        }

        // Разделяем по запятым. Для определения типа события смотрим первое поле.
        let mut fields = line.split(',').map(str::trim);
        let event_type = match fields.next() {
            Some(field) => field.trim_matches('"').trim(),
            None => continue,
        };

        // В .osu тип перерыва обозначается как число 2 или слово Break (без учёта регистра).
        if event_type != "2" && !event_type.eq_ignore_ascii_case("break") {
            continue;
        }

        if breaks.len() == MAX_BREAKS {
            return Err(RoseaError::ResourceLimit {
                resource: "breaks",
                limit: MAX_BREAKS as u64,
                observed: breaks.len() as u64 + 1,
            });
        }

        let line_number = index + 1;
        let invalid = |field| RoseaError::InvalidBreakField {
            field,
            line: line_number,
        };

        let start_str = fields
            .next()
            .ok_or_else(|| invalid("startTime"))?
            .trim_matches('"')
            .trim();
        let start_time: f64 = start_str.parse().map_err(|_| invalid("startTime"))?;
        if !start_time.is_finite() {
            return Err(invalid("startTime"));
        }

        let end_str = fields
            .next()
            .ok_or_else(|| invalid("endTime"))?
            .trim_matches('"')
            .trim();
        let end_time: f64 = end_str.parse().map_err(|_| invalid("endTime"))?;
        if !end_time.is_finite() {
            return Err(invalid("endTime"));
        }

        if fields.next().is_some() {
            return Err(invalid("fields"));
        }

        // total_cmp считает оба написания нуля одним временем.
        let start_time = if start_time == 0.0 { 0.0 } else { start_time };
        let end_time = if end_time == 0.0 { 0.0 } else { end_time };

        // Отрицательную продолжительность перерыва нормализуем до 0 (end = max(start, end)).
        let end_time = if end_time < start_time {
            start_time
        } else {
            end_time
        };

        breaks.push(BreakPeriod {
            start_time,
            end_time,
        });
    }

    // Сортируем перерывы по времени начала (sort_by сохраняет порядок при равных временах).
    breaks.sort_by(|a, b| a.start_time.total_cmp(&b.start_time));
    Ok(breaks)
}
