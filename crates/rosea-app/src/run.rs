//! Завершённый headless-прогон с общим загрузчиком.

use std::path::Path;

use rosea_core::{
    model::{ExpectedStats, ReplayMeta, Stats, Warning},
    prepare::prepare_map,
    sim::Simulator,
};

use crate::{AppError, load_pair};

/// Итог завершённого прогона и данные реплея для отчёта.
#[derive(Debug)]
pub struct HeadlessRun {
    pub actual: Stats,
    pub expected: ExpectedStats,
    pub meta: ReplayMeta,
    pub warnings: Vec<Warning>,
}

/// Загружает пару, готовит карту один раз и проводит симуляцию до конца.
pub fn run_headless<'a>(map: &'a Path, replay: &'a Path) -> Result<HeadlessRun, AppError<'a>> {
    let pair = load_pair(map, replay)?;
    let prepared = prepare_map(&pair.map).map_err(|source| AppError::Prepare {
        path: map,
        source: source.into(),
    })?;
    let actual = Simulator::new(&prepared, &pair.input)
        .and_then(Simulator::finish)
        .map_err(|source| AppError::Simulate {
            path: map,
            source: source.into(),
        })?;
    Ok(HeadlessRun {
        actual,
        expected: pair.expected,
        meta: pair.meta,
        warnings: pair.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosea_core::{error::RoseaError, model::ObjectId};

    #[test]
    fn simulate_error_keeps_core_code_and_position() {
        let error = AppError::Simulate {
            path: Path::new("карта.osu"),
            source: RoseaError::NumericalError {
                line: 12,
                object_id: ObjectId(2),
            }
            .into(),
        };
        assert!(
            error
                .to_string()
                .starts_with("NumericalError [simulate] \"карта.osu\" line=12 object_id=2: ")
        );
    }
}
