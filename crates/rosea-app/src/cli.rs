//! Разбор аргументов CLI без преобразования путей в UTF-8.

use std::{ffi::OsString, path::PathBuf};

pub(crate) const HELP: &str = "Использование: rosea inspect --map ПУТЬ.osu --replay ПУТЬ.osr\n\
    rosea --help    Показать справку\n";

pub(crate) enum Command {
    Help,
    Inspect { map: PathBuf, replay: PathBuf },
}

pub(crate) struct UsageError(pub(crate) &'static str);

pub(crate) fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command, UsageError> {
    let command = args.next().ok_or(UsageError("Укажите команду inspect"))?;
    if command == "--help" && args.next().is_none() {
        return Ok(Command::Help);
    }
    if command != "inspect" {
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
        } else {
            return Err(UsageError(
                "Неизвестный флаг или лишний аргумент команды inspect",
            ));
        };
        if slot.is_some() {
            return Err(UsageError(duplicate));
        }
        let value = args
            .next()
            .filter(|arg| !arg.is_empty() && !arg.as_encoded_bytes().starts_with(b"--"))
            .ok_or(UsageError(missing))?;
        *slot = Some(PathBuf::from(value));
    }

    Ok(Command::Inspect {
        map: map.ok_or(UsageError("Обязателен флаг --map"))?,
        replay: replay.ok_or(UsageError("Обязателен флаг --replay"))?,
    })
}
