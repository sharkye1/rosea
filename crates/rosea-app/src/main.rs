//! Точка входа приложения.

#![forbid(unsafe_code)]

mod cli;
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
            let bytes = match load::read_limited(&map, load::MAP_LIMIT) {
                Ok(bytes) => bytes,
                Err(error) => return fail(error),
            };
            fail(format_args!(
                "InspectUnavailable: карта {map:?} прочитана ({} байт), загрузка реплея {replay:?} пока недоступна",
                bytes.len()
            ))
        }
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
