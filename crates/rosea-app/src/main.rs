//! Точка входа приложения.

#![forbid(unsafe_code)]

mod cli;

use std::{
    env,
    fmt::Display,
    io::{self, Write},
    process::ExitCode,
};

use rosea_app::{LoadedPair, load_pair};
use rosea_core::model::Warning;

fn main() -> ExitCode {
    match cli::parse(env::args_os().skip(1)) {
        Ok(cli::Command::Help) => write_stdout(|out| out.write_all(cli::HELP.as_bytes())),
        Ok(cli::Command::Inspect { map, replay }) => {
            let pair = match load_pair(&map, &replay) {
                Ok(pair) => pair,
                Err(error) => return fail(error),
            };
            write_stdout(|out| write_inspection(out, &pair))
        }
        Ok(cli::Command::Play) if !cfg!(feature = "window") => {
            fail("WindowUnavailable: пересоберите rosea-app с feature window для оконного режима")
        }
        Ok(cli::Command::Play) => fail("PlayUnavailable: оконный режим пока недоступен"),
        Err(cli::UsageError(message)) => {
            fail(format_args!("UsageError: {message}. Справка: rosea --help"))
        }
    }
}

fn write_inspection(out: &mut impl Write, pair: &LoadedPair) -> io::Result<()> {
    let counts = pair.map.object_counts();
    writeln!(
        out,
        "MD5 карты: {}",
        pair.meta.beatmap_hash().to_ascii_lowercase()
    )?;
    writeln!(
        out,
        "Версия клиента (game_version): {}",
        pair.meta.game_version()
    )?;
    writeln!(out, "Объектов: {}", pair.map.object_count())?;
    writeln!(
        out,
        "Круги: {}, слайдеры L: {}, B: {}, P: {}, спиннеры: {}",
        counts.circles, counts.sliders_l, counts.sliders_b, counts.sliders_p, counts.spinners
    )?;
    writeln!(
        out,
        "Нормализованных записей: {}",
        pair.input.record_count()
    )?;
    writeln!(out, "Предупреждений: {}", pair.warnings.len())?;
    for warning in &pair.warnings {
        match warning {
            Warning::UnknownBeatmapFields { count } => writeln!(
                out,
                "UnknownBeatmapFields: неизвестных полей и секций карты {count}"
            )?,
            Warning::ReplayTrailerNotInterpreted => writeln!(
                out,
                "ReplayTrailerNotInterpreted: хвост реплея сохранён без интерпретации"
            )?,
            Warning::UnknownReplayKeys { bits, records } => writeln!(
                out,
                "UnknownReplayKeys: неизвестные биты кнопок {bits:#x}, записей {records}"
            )?,
            Warning::ReplayInputNormalized {
                changed_times,
                initial_dummy_removed,
                backward_removed,
                ..
            } => writeln!(
                out,
                "ReplayInputNormalized: изменено времён {changed_times}, удалено служебных записей {initial_dummy_removed}, удалено записей с обратным ходом времени {backward_removed}"
            )?,
        }
    }
    Ok(())
}

fn write_stdout(write: impl FnOnce(&mut io::StdoutLock<'_>) -> io::Result<()>) -> ExitCode {
    let mut stdout = io::stdout().lock();
    match write(&mut stdout).and_then(|()| stdout.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(format_args!("IoError [write]: {error}")),
    }
}

fn fail(message: impl Display) -> ExitCode {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{message}");
    let _ = stderr.flush();
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_smoke() {
        assert_eq!(rosea_core::PROFILE_ID, "rosea-mvp-v1");
    }
}
