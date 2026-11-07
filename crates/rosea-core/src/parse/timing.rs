//! Чтение обычных и inherited-точек; применение BPM и скорости относится к подготовке карты.

use crate::error::RoseaError;
use crate::model::TimingPoint;

use super::osu_mode;

const MAX_TIMING_POINTS: usize = 100_000;
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Читает обычные и inherited timing points с сохранением исходных значений полей.
/// Пустая секция допустима. Результат упорядочен по времени и исходной строке.
pub fn osu_timing_points(bytes: &[u8]) -> Result<Vec<TimingPoint>, RoseaError> {
    osu_mode(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    let mut in_timing = false;
    let mut points = Vec::new();

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
            in_timing = line == "[TimingPoints]";
            continue;
        }
        if !in_timing {
            continue;
        }
        // Проверяем до push, чтобы вход не увеличивал Vec сверх лимита.
        if points.len() == MAX_TIMING_POINTS {
            return Err(RoseaError::ResourceLimit {
                resource: "timing_points",
                limit: MAX_TIMING_POINTS as u64,
                observed: points.len() as u64 + 1,
            });
        }
        points.push(parse_timing_point(line, index + 1)?);
    }

    // При одинаковом времени сохраняем порядок объявлений в карте.
    points.sort_by(|a, b| a.time.total_cmp(&b.time).then_with(|| a.line.cmp(&b.line)));
    Ok(points)
}

fn parse_timing_point(line: &str, line_number: usize) -> Result<TimingPoint, RoseaError> {
    let invalid = |field| RoseaError::InvalidTimingPointField {
        field,
        line: line_number,
    };
    let mut fields = line.split(',').map(str::trim);
    // Восемь полей помещаются в массив без отдельного Vec.
    // zip связывает каждую ячейку с именем поля для диагностики пропуска.
    let mut values = [""; 8];
    for (slot, field) in values.iter_mut().zip([
        "time",
        "beatLength",
        "meter",
        "sampleSet",
        "sampleIndex",
        "volume",
        "uninherited",
        "effects",
    ]) {
        *slot = fields.next().ok_or_else(|| invalid(field))?;
    }
    if fields.next().is_some() {
        return Err(invalid("fields"));
    }

    let time: f64 = values[0].parse().map_err(|_| invalid("time"))?;
    if !time.is_finite() {
        return Err(invalid("time"));
    }
    // 1 задаёт обычную точку темпа, 0 — inherited-точку изменения скорости.
    let uninherited: u8 = values[6].parse().map_err(|_| invalid("uninherited"))?;
    let uninherited = match uninherited {
        0 => false,
        1 => true,
        _ => return Err(invalid("uninherited")),
    };
    // У обычной точки beatLength положителен. У inherited сохраняем любой конечный:
    // отрицательный задаёт обратный множитель, неотрицательный сбрасывает скорость.
    let beat_length: f64 = values[1].parse().map_err(|_| invalid("beatLength"))?;
    if !beat_length.is_finite() || (uninherited && beat_length <= 0.0) {
        return Err(invalid("beatLength"));
    }
    let meter: u32 = values[2].parse().map_err(|_| invalid("meter"))?;
    // Размер такта применяется только к обычным точкам; у inherited допускаем 0.
    if uninherited && meter == 0 {
        return Err(invalid("meter"));
    }
    let sample_set: u8 = values[3].parse().map_err(|_| invalid("sampleSet"))?;
    if sample_set > 3 {
        return Err(invalid("sampleSet"));
    }
    let sample_index: u32 = values[4].parse().map_err(|_| invalid("sampleIndex"))?;
    let volume: u8 = values[5].parse().map_err(|_| invalid("volume"))?;
    if volume > 100 {
        return Err(invalid("volume"));
    }
    // Сохраняем всю маску, включая неиспользуемые биты.
    let effects: u32 = values[7].parse().map_err(|_| invalid("effects"))?;

    // total_cmp различает -0.0 и +0.0; приводим оба к +0.0 перед сортировкой.
    Ok(TimingPoint {
        line: line_number,
        time: if time == 0.0 { 0.0 } else { time },
        beat_length,
        meter,
        sample_set,
        sample_index,
        volume,
        uninherited,
        effects,
    })
}
