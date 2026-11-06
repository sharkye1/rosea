use crate::error::RoseaError;
use crate::model::{Circle, ObjectId, Point};

use super::osu_mode;

const MAX_OBJECTS: usize = 100_000;
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Читает карту с кругами; другие типы объектов возвращают ошибку.
pub fn osu_circles(bytes: &[u8]) -> Result<Vec<Circle>, RoseaError> {
    osu_mode(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;
    let mut in_objects = false;
    let mut circles = Vec::new();

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
        if circles.len() == MAX_OBJECTS {
            return Err(RoseaError::ResourceLimit {
                resource: "hit_objects",
                limit: MAX_OBJECTS as u64,
                observed: circles.len() as u64 + 1,
            });
        }
        let id = ObjectId(circles.len() as u32);
        circles.push(parse_circle(line, index + 1, id)?);
    }

    if circles.is_empty() {
        return Err(RoseaError::EmptyBeatmap);
    }
    circles.sort_by(|a, b| {
        a.start_time
            .total_cmp(&b.start_time)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });
    Ok(circles)
}

fn parse_circle(line: &str, line_number: usize, id: ObjectId) -> Result<Circle, RoseaError> {
    let invalid = |field| RoseaError::InvalidHitObjectField {
        field,
        line: line_number,
        object_id: id,
    };
    // Хвост с именем звукового файла может содержать запятые.
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
    if flags & (1 | 2 | 8 | 128) != 1 {
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
    if let Some(sample) = fields.next().filter(|sample| !sample.is_empty()) {
        let mut sample_fields = sample.splitn(5, ':');
        for _ in 0..4 {
            sample_fields
                .next()
                .unwrap_or("")
                .parse::<u32>()
                .map_err(|_| invalid("hitSample"))?;
        }
        if sample_fields.next().is_none() {
            return Err(invalid("hitSample"));
        }
    }
    Ok(Circle {
        id,
        line: line_number,
        position: Point { x, y },
        start_time: if start_time == 0.0 { 0.0 } else { start_time },
        flags,
        hit_sound,
    })
}
