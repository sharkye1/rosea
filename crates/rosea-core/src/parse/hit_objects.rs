//! Чтение кругов, L/B/P-слайдеров и спиннеров. Построение пути и времён относится к prepare.

use crate::error::RoseaError;
use crate::model::{CurveKind, HitObject, HitObjectKind, ObjectId, Point};

use super::osu_mode;

const MAX_OBJECTS: usize = 100_000;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_CONTROLS: usize = 4096;
const MAX_SPANS: u32 = 10_000;

/// Читает смешанный список кругов, L/B/P-слайдеров и спиннеров по (start_time, исходный ID).
pub fn osu_hit_objects(bytes: &[u8]) -> Result<Vec<HitObject>, RoseaError> {
    parse_objects(bytes, true)
}

// Старый вход osu_circles использует тот же разбор, сохраняя отказ на слайдерах и спиннерах.
pub(super) fn parse_objects(
    bytes: &[u8],
    allow_complex: bool,
) -> Result<Vec<HitObject>, RoseaError> {
    osu_mode(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    let mut in_objects = false;
    let mut objects = Vec::new();
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
            in_objects = line == "[HitObjects]";
            continue;
        }
        if !in_objects {
            continue;
        }
        if objects.len() == MAX_OBJECTS {
            return Err(RoseaError::ResourceLimit {
                resource: "hit_objects",
                limit: MAX_OBJECTS as u64,
                observed: objects.len() as u64 + 1,
            });
        }
        // ID назначается до сортировки и общий для кругов, слайдеров и спиннеров.
        let id = ObjectId(objects.len() as u32);
        objects.push(parse_object(line, index + 1, id, allow_complex)?);
    }
    if objects.is_empty() {
        return Err(RoseaError::EmptyBeatmap);
    }
    objects.sort_by(|a, b| {
        a.start_time
            .total_cmp(&b.start_time)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });
    Ok(objects)
}

fn parse_object(
    line: &str,
    line_number: usize,
    id: ObjectId,
    allow_complex: bool,
) -> Result<HitObject, RoseaError> {
    let invalid = |field| RoseaError::InvalidHitObjectField {
        field,
        line: line_number,
        object_id: id,
    };
    // Первые пять полей общие. Весь хвост оставляем для конкретного типа объекта.
    let mut fields = line.splitn(6, ',').map(str::trim);
    let x: f32 = fields
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| invalid("x"))?;
    let y: f32 = fields
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| invalid("y"))?;
    let start_time: f64 = fields
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| invalid("time"))?;
    let flags: u8 = fields
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| invalid("type"))?;
    // Флаги new-combo и цвета не входят в маску базового типа.
    let base_type = flags & (1 | 2 | 8 | 128);
    let is_circle = base_type == 1;
    let is_slider = allow_complex && base_type == 2;
    let is_spinner = allow_complex && base_type == 8;
    if !is_circle && !is_slider && !is_spinner {
        return Err(RoseaError::UnsupportedHitObject {
            line: line_number,
            object_id: id,
            flags,
        });
    }
    let hit_sound: u8 = fields
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| invalid("hitSound"))?;
    for (field, finite) in [
        ("x", x.is_finite()),
        ("y", y.is_finite()),
        ("time", start_time.is_finite()),
    ] {
        if !finite {
            return Err(invalid(field));
        }
    }
    let position = Point { x, y };
    let tail = fields.next().unwrap_or("");
    let kind = if is_circle {
        validate_hit_sample(tail, line_number, id)?;
        HitObjectKind::Circle
    } else if is_slider {
        parse_slider(tail, position, line_number, id, flags)?
    } else {
        parse_spinner(tail, start_time, line_number, id)?
    };
    Ok(HitObject {
        id,
        line: line_number,
        position,
        // total_cmp должен считать оба написания нуля одним временем.
        start_time: if start_time == 0.0 { 0.0 } else { start_time },
        flags,
        hit_sound,
        kind,
    })
}

fn parse_spinner(
    tail: &str,
    start_time: f64,
    line: usize,
    id: ObjectId,
) -> Result<HitObjectKind, RoseaError> {
    let invalid = |field| RoseaError::InvalidHitObjectField {
        field,
        line,
        object_id: id,
    };
    if tail.is_empty() {
        return Err(invalid("endTime"));
    }
    let (end_str, sample_str) = match tail.split_once(',') {
        Some((end, sample)) => (end.trim(), sample.trim()),
        None => (tail.trim(), ""),
    };
    let end: f64 = end_str.parse().map_err(|_| invalid("endTime"))?;
    if !end.is_finite() || end < start_time {
        return Err(invalid("endTime"));
    }
    validate_hit_sample(sample_str, line, id)?;
    Ok(HitObjectKind::Spinner {
        end: if end == 0.0 { 0.0 } else { end },
    })
}

fn parse_slider(
    tail: &str,
    position: Point,
    line: usize,
    id: ObjectId,
    flags: u8,
) -> Result<HitObjectKind, RoseaError> {
    let invalid = |field| RoseaError::InvalidHitObjectField {
        field,
        line,
        object_id: id,
    };
    // Последний hitSample может содержать запятые в имени файла.
    let mut fields = tail.splitn(6, ',').map(str::trim);
    let mut curve = fields.next().unwrap_or("").split('|').map(str::trim);
    let kind = match curve.next().unwrap_or("") {
        "" => return Err(invalid("curveType")),
        "L" => CurveKind::Linear,
        "B" => CurveKind::Bezier,
        "P" => CurveKind::PerfectCurve,
        value if value.starts_with(['L', 'B', 'P', 'C']) && value.len() > 1 => {
            return Err(RoseaError::UnsupportedSliderSyntax {
                line,
                object_id: id,
            });
        }
        _ => {
            return Err(RoseaError::UnsupportedHitObject {
                line,
                object_id: id,
                flags,
            });
        }
    };
    let spans: u32 = fields
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| invalid("slides"))?;
    if spans == 0 {
        return Err(invalid("slides"));
    }
    if spans > MAX_SPANS {
        return Err(RoseaError::ResourceLimit {
            resource: "slider_spans",
            limit: u64::from(MAX_SPANS),
            observed: u64::from(spans),
        });
    }
    let declared_length: f64 = fields
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| invalid("length"))?;
    if !declared_length.is_finite() || declared_length < 0.0 {
        return Err(invalid("length"));
    }
    // Голова — первая контрольная точка. Лимит учитывает и её, до роста Vec.
    let mut controls = vec![position];
    for point in curve {
        // Маркеры типов внутри пути относятся к расширенному формату, а не к координатам.
        if point.starts_with(['L', 'B', 'P', 'C']) {
            return Err(RoseaError::UnsupportedSliderSyntax {
                line,
                object_id: id,
            });
        }
        if controls.len() == MAX_CONTROLS {
            return Err(RoseaError::ResourceLimit {
                resource: "slider_control_points",
                limit: MAX_CONTROLS as u64,
                observed: controls.len() as u64 + 1,
            });
        }
        let (x, y) = point
            .split_once(':')
            .ok_or_else(|| invalid("curvePoints"))?;
        let x: f32 = x.trim().parse().map_err(|_| invalid("curvePoints"))?;
        let y: f32 = y.trim().parse().map_err(|_| invalid("curvePoints"))?;
        if !x.is_finite() || !y.is_finite() {
            return Err(invalid("curvePoints"));
        }
        controls.push(Point { x, y });
    }
    if controls.len() < 2 {
        return Err(invalid("curvePoints"));
    }
    // Звуковые метаданные проверяем по форме, файлы ресурсов не открываем.
    for (field, edges) in [
        ("edgeSounds", fields.next().unwrap_or("")),
        ("edgeSets", fields.next().unwrap_or("")),
    ] {
        if edges.is_empty() {
            continue;
        }
        for edge in edges.split('|') {
            if field == "edgeSounds" {
                edge.trim().parse::<u8>().map_err(|_| invalid(field))?;
            } else {
                let (normal, addition) = edge.split_once(':').ok_or_else(|| invalid(field))?;
                let normal: u8 = normal.trim().parse().map_err(|_| invalid(field))?;
                let addition: u8 = addition.trim().parse().map_err(|_| invalid(field))?;
                if normal > 3 || addition > 3 {
                    return Err(invalid(field));
                }
            }
        }
    }
    validate_hit_sample(fields.next().unwrap_or(""), line, id)?;
    Ok(HitObjectKind::Slider {
        kind,
        controls,
        spans,
        declared_length: if declared_length == 0.0 {
            0.0
        } else {
            declared_length
        },
    })
}

fn validate_hit_sample(sample: &str, line: usize, id: ObjectId) -> Result<(), RoseaError> {
    if sample.is_empty() {
        return Ok(());
    }
    let invalid = || RoseaError::InvalidHitObjectField {
        field: "hitSample",
        line,
        object_id: id,
    };
    let mut fields = sample.splitn(5, ':');
    for _ in 0..4 {
        fields
            .next()
            .unwrap_or("")
            .parse::<u32>()
            .map_err(|_| invalid())?;
    }
    if fields.next().is_none() {
        return Err(invalid());
    }
    Ok(())
}
