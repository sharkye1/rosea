//! Точка входа приложения.

#![forbid(unsafe_code)]

mod cli;
mod hash;
mod load;

use std::{
    env,
    fmt::Display,
    io::{self, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    match cli::parse(env::args_os().skip(1)) {
        Ok(cli::Command::Help) => {
            let mut stdout = io::stdout().lock();
            match stdout
                .write_all(cli::HELP.as_bytes())
                .and_then(|()| stdout.flush())
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => fail(format_args!("IoError [write]: {error}")),
            }
        }
        Ok(cli::Command::Inspect { map, replay }) => {
            let pair = match load::load_pair(&map, &replay) {
                Ok(pair) => pair,
                Err(error) => return fail(error),
            };
            let _expected = pair.expected;
            fail(format_args!(
                "InspectUnavailable: пара проверена (объектов {}, записей {}, версия {}, предупреждений {}), вывод сводки пока недоступен",
                pair.map.object_count(),
                pair.input.record_count(),
                pair.meta.game_version(),
                pair.warnings.len()
            ))
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
