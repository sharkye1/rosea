//! Разбор аргументов CLI без преобразования путей в UTF-8.

use std::{ffi::OsString, path::PathBuf};

pub(crate) const HELP: &str = "Использование: rosea inspect --map ПУТЬ.osu --replay ПУТЬ.osr\n\
    rosea play --map ПУТЬ.osu --replay ПУТЬ.osr [--width W] [--height H]\n\
    rosea --help    Показать справку\n";

pub(crate) enum Command {
    Help,
    Inspect { map: PathBuf, replay: PathBuf },
    Play,
}

pub(crate) struct UsageError(pub(crate) &'static str);

pub(crate) fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command, UsageError> {
    let command = args
        .next()
        .ok_or(UsageError("Укажите команду inspect или play"))?;
    if command == "--help" && args.next().is_none() {
        return Ok(Command::Help);
    }
    let play = command == "play";
    if command != "inspect" && !play {
        return Err(UsageError("Неизвестная команда"));
    }
    let mut args = args.peekable();
    if args.peek().is_some_and(|arg| arg == "--help") {
        args.next();
        return if args.next().is_none() {
            Ok(Command::Help)
        } else {
            Err(UsageError("После --help не должно быть аргументов"))
        };
    }

    let mut map = None;
    let mut replay = None;
    let mut width = None;
    let mut height = None;
    while let Some(flag) = args.next() {
        let (slot, duplicate, missing) = if flag == "--map" {
            (
                &mut map,
                "Флаг --map указан повторно",
                "Укажите путь после --map",
            )
        } else if flag == "--replay" {
            (
                &mut replay,
                "Флаг --replay указан повторно",
                "Укажите путь после --replay",
            )
        } else if play && flag == "--width" {
            (
                &mut width,
                "Флаг --width указан повторно",
                "Укажите ширину после --width",
            )
        } else if play && flag == "--height" {
            (
                &mut height,
                "Флаг --height указан повторно",
                "Укажите высоту после --height",
            )
        } else {
            return Err(UsageError("Неизвестный флаг или лишний аргумент"));
        };
        if slot.is_some() {
            return Err(UsageError(duplicate));
        }
        let value = args
            .next()
            .filter(|arg| !arg.is_empty() && !arg.as_encoded_bytes().starts_with(b"--"))
            .ok_or(UsageError(missing))?;
        *slot = Some(value);
    }

    let map = map.ok_or(UsageError("Обязателен флаг --map"))?;
    let replay = replay.ok_or(UsageError("Обязателен флаг --replay"))?;
    if play {
        let width = dimension(width, 1024)?;
        let height = dimension(height, 768)?;
        if width * height > 8_388_608 {
            return Err(UsageError(
                "Площадь окна не должна превышать 8388608 пикселей",
            ));
        }
        return Ok(Command::Play);
    }
    Ok(Command::Inspect {
        map: PathBuf::from(map),
        replay: PathBuf::from(replay),
    })
}

fn dimension(argument: Option<OsString>, default: u32) -> Result<u32, UsageError> {
    match argument {
        None => Ok(default),
        Some(value) => value
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| (64..=4096).contains(value))
            .ok_or(UsageError(
                "Размер окна должен быть целым числом от 64 до 4096",
            )),
    }
}
