//! Разбор аргументов CLI без преобразования путей в UTF-8.

use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::PathBuf,
};

use rosea_core::model::TimeMs;

pub(crate) const HELP: &str = "Использование: rosea inspect --map ПУТЬ.osu --replay ПУТЬ.osr\n\
    rosea play --map ПУТЬ.osu --replay ПУТЬ.osr [--width W] [--height H]\n\
    rosea frame --map ПУТЬ.osu --replay ПУТЬ.osr --at-ms T --out КАДР.ppm [--width W] [--height H]\n\
    rosea export --map ПУТЬ.osu --replay ПУТЬ.osr --from-ms T0 --to-ms T1 --out ФАЙЛ.y4m [--fps F] [--width W] [--height H]\n\
    Вывод кадров и видео пока недоступен.\n\
    rosea --help    Показать справку\n";

pub(crate) enum Command {
    Help,
    Inspect { map: PathBuf, replay: PathBuf },
    Play,
    Frame,
    Export,
}

#[derive(Debug)]
pub(crate) enum ArgumentError {
    Usage(&'static str),
    Limit {
        resource: &'static str,
        limit: u32,
        observed: String,
    },
}

use ArgumentError::Usage;

impl fmt::Display for ArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "UsageError: {message}. Справка: rosea --help"),
            Self::Limit {
                resource,
                limit,
                observed,
            } => write!(
                f,
                "ResourceLimit [parse]: превышен лимит ресурса {resource}, limit={limit}, observed={observed}"
            ),
        }
    }
}

pub(crate) fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command, ArgumentError> {
    let command = args
        .next()
        .ok_or(Usage("Укажите команду inspect, play, frame или export"))?;
    if command == "--help" && args.next().is_none() {
        return Ok(Command::Help);
    }
    let play = command == "play";
    let frame = command == "frame";
    let export = command == "export";
    let image = play || frame || export;
    if command != "inspect" && !image {
        return Err(Usage("Неизвестная команда"));
    }
    let mut args = args.peekable();
    if args.peek().is_some_and(|arg| arg == "--help") {
        args.next();
        return if args.next().is_none() {
            Ok(Command::Help)
        } else {
            Err(Usage("После --help не должно быть аргументов"))
        };
    }

    let mut map = None;
    let mut replay = None;
    let mut width = None;
    let mut height = None;
    let mut at = None;
    let mut from = None;
    let mut to = None;
    let mut fps = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let (slot, duplicate, missing) = match flag.as_encoded_bytes() {
            b"--map" => (
                &mut map,
                "Флаг --map указан повторно",
                "Укажите путь после --map",
            ),
            b"--replay" => (
                &mut replay,
                "Флаг --replay указан повторно",
                "Укажите путь после --replay",
            ),
            b"--width" if image => (
                &mut width,
                "Флаг --width указан повторно",
                "Укажите ширину после --width",
            ),
            b"--height" if image => (
                &mut height,
                "Флаг --height указан повторно",
                "Укажите высоту после --height",
            ),
            b"--at-ms" if frame => (
                &mut at,
                "Флаг --at-ms указан повторно",
                "Укажите время кадра после --at-ms",
            ),
            b"--from-ms" if export => (
                &mut from,
                "Флаг --from-ms указан повторно",
                "Укажите начальное время после --from-ms",
            ),
            b"--to-ms" if export => (
                &mut to,
                "Флаг --to-ms указан повторно",
                "Укажите конечное время после --to-ms",
            ),
            b"--fps" if export => (
                &mut fps,
                "Флаг --fps указан повторно",
                "Укажите частоту кадров после --fps",
            ),
            b"--out" if frame || export => (
                &mut out,
                "Флаг --out указан повторно",
                "Укажите путь вывода после --out",
            ),
            _ => return Err(Usage("Неизвестный флаг или лишний аргумент")),
        };
        if slot.is_some() {
            return Err(Usage(duplicate));
        }
        let value = args
            .next()
            .filter(|arg| !arg.is_empty() && !arg.as_encoded_bytes().starts_with(b"--"))
            .ok_or(Usage(missing))?;
        *slot = Some(value);
    }

    let map = map.ok_or(Usage("Обязателен флаг --map"))?;
    let replay = replay.ok_or(Usage("Обязателен флаг --replay"))?;
    if play {
        image_size(width.as_deref(), height.as_deref()).map_err(|error| match error {
            ArgumentError::Limit {
                resource: "image_pixels",
                ..
            } => Usage("Площадь окна не должна превышать 8388608 пикселей"),
            _ => Usage("Размер окна должен быть целым числом от 64 до 4096"),
        })?;
        return Ok(Command::Play);
    }
    if frame || export {
        out.ok_or(Usage("Обязателен флаг --out"))?;
        image_size(width.as_deref(), height.as_deref())?;
        if frame {
            time(at, "Обязателен флаг --at-ms")?;
            return Ok(Command::Frame);
        }
        let from = time(from, "Обязателен флаг --from-ms")?;
        let to = time(to, "Обязателен флаг --to-ms")?;
        if from.get() >= to.get() {
            return Err(Usage("Время --from-ms должно быть меньше --to-ms"));
        }
        export_fps(fps.as_deref())?;
        return Ok(Command::Export);
    }
    Ok(Command::Inspect {
        map: PathBuf::from(map),
        replay: PathBuf::from(replay),
    })
}

fn image_size(width: Option<&OsStr>, height: Option<&OsStr>) -> Result<(u32, u32), ArgumentError> {
    const DIMENSION_ERROR: &str = "Размер изображения должен быть целым числом от 64 до 4096";
    const MAX_AREA: u32 = 8_388_608;
    let width = bounded_integer(width, 1024, 64, 4096, "image_width", DIMENSION_ERROR)?;
    let height = bounded_integer(height, 768, 64, 4096, "image_height", DIMENSION_ERROR)?;
    // Оба множителя уже ограничены 4096, произведение помещается в u32.
    let area = width * height;
    if area > MAX_AREA {
        return Err(resource_limit("image_pixels", MAX_AREA, area));
    }
    Ok((width, height))
}

fn export_fps(argument: Option<&OsStr>) -> Result<u32, ArgumentError> {
    bounded_integer(
        argument,
        60,
        1,
        240,
        "export_fps",
        "Частота кадров должна быть целым числом от 1 до 240",
    )
}

fn bounded_integer(
    argument: Option<&OsStr>,
    default: u32,
    min: u32,
    max: u32,
    resource: &'static str,
    message: &'static str,
) -> Result<u32, ArgumentError> {
    let Some(argument) = argument else {
        return Ok(default);
    };
    let observed = ascii(argument)
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|&value| value >= u64::from(min))
        .ok_or(Usage(message))?;
    if observed > u64::from(max) {
        return Err(resource_limit(resource, max, observed));
    }
    Ok(observed as u32)
}

fn time(argument: Option<OsString>, missing: &'static str) -> Result<TimeMs, ArgumentError> {
    const MAX_TIME: u32 = 86_400_000;
    let argument = argument.ok_or(Usage(missing))?;
    let time = ascii(&argument)
        .and_then(|text| text.parse::<f64>().ok())
        .and_then(|ms| TimeMs::new(ms).ok())
        .ok_or(Usage("Время должно быть конечным числом в миллисекундах"))?;
    let magnitude = time.get().abs();
    if magnitude > f64::from(MAX_TIME) {
        return Err(resource_limit("absolute_time_ms", MAX_TIME, magnitude));
    }
    Ok(time)
}

fn ascii(argument: &OsStr) -> Option<&str> {
    argument.to_str().filter(|text| text.is_ascii())
}

fn resource_limit(
    resource: &'static str,
    limit: u32,
    observed: impl fmt::Display,
) -> ArgumentError {
    ArgumentError::Limit {
        resource,
        limit,
        observed: observed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_defaults_match_the_cli_contract() {
        assert_eq!(image_size(None, None).unwrap(), (1024, 768));
        assert_eq!(export_fps(None).unwrap(), 60);
    }
}
