//! Точка входа приложения.

#![forbid(unsafe_code)]

mod cli;

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
        Ok(cli::Command::Inspect { map, replay }) => fail(format_args!(
            "InspectUnavailable: загрузка пары пока недоступна ({:?}, {:?})",
            map, replay
        )),
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
