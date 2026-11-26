//! Чтение явно заданных локальных файлов с ограничением размера.

use rosea_core::{
    error::{ErrorCode, ErrorWithLocation, RoseaError, SourceLocation},
    model::{Beatmap, ExpectedStats, ReplayMeta, Warning},
    parse::{beatmap_md5, osr_with_location, osu},
    replay::{ReplayTimeline, normalize_with_location},
};
use std::{
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};

use crate::hash;

const MAP_LIMIT: u64 = 16 * 1024 * 1024;
const REPLAY_LIMIT: u64 = 64 * 1024 * 1024;

/// Проверенная пара с нормализованным вводом.
pub struct LoadedPair {
    pub map: Beatmap,
    pub input: ReplayTimeline,
    pub meta: ReplayMeta,
    pub expected: ExpectedStats,
    pub warnings: Vec<Warning>,
}

#[derive(Debug)]
pub enum AppError<'a> {
    Read(ReadError<'a>),
    Parse {
        path: &'a Path,
        source: ErrorWithLocation,
    },
    HashMismatch {
        map: &'a Path,
        replay: &'a Path,
    },
    Normalize {
        path: &'a Path,
        source: ErrorWithLocation,
    },
    Prepare {
        path: &'a Path,
        source: ErrorWithLocation,
    },
    Simulate {
        path: &'a Path,
        source: ErrorWithLocation,
    },
}

impl fmt::Display for AppError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(f),
            Self::Parse { path, source } => fmt_core_error(f, "parse", path, source),
            Self::HashMismatch { map, replay } => write!(
                f,
                "BeatmapHashMismatch [parse]: карта {map:?} не соответствует реплею {replay:?}"
            ),
            Self::Normalize {
                path,
                source:
                    ErrorWithLocation {
                        error: RoseaError::NoReplayData { .. },
                        ..
                    },
            } => write!(
                f,
                "{} [normalize] {path:?}: после нормализации не осталось записей реплея",
                ErrorCode::NoReplayData,
            ),
            Self::Normalize { path, source } => fmt_core_error(f, "normalize", path, source),
            Self::Prepare { path, source } => fmt_core_error(f, "prepare", path, source),
            Self::Simulate { path, source } => fmt_core_error(f, "simulate", path, source),
        }
    }
}

impl std::error::Error for AppError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => error.source(),
            Self::Parse { source, .. }
            | Self::Normalize { source, .. }
            | Self::Prepare { source, .. }
            | Self::Simulate { source, .. } => Some(source),
            Self::HashMismatch { .. } => None,
        }
    }
}

fn fmt_core_error(
    f: &mut fmt::Formatter<'_>,
    stage: &str,
    path: &Path,
    source: &ErrorWithLocation,
) -> fmt::Result {
    write!(f, "{} [{stage}] {path:?}", source.error.code())?;
    match source.location {
        SourceLocation::None => {}
        SourceLocation::Line { line, object_id } => {
            write!(f, " line={line}")?;
            if let Some(id) = object_id {
                write!(f, " object_id={}", id.0)?;
            }
        }
        SourceLocation::ByteOffset(offset) => write!(f, " byte_offset={offset}")?,
        SourceLocation::ReplayRecord(index) => write!(f, " record_index={index}")?,
        SourceLocation::Object(id) => write!(f, " object_id={}", id.0)?,
    }
    match &source.error {
        RoseaError::ResourceLimit {
            resource,
            limit,
            observed,
        } => fmt_limit(f, resource, *limit, observed),
        RoseaError::MapResourceLimit {
            resource,
            limit,
            observed,
            ..
        } => fmt_limit(f, resource, *limit, observed),
        _ => write!(f, ": {source}"),
    }
}

fn fmt_limit(
    f: &mut fmt::Formatter<'_>,
    resource: &str,
    limit: u64,
    observed: impl fmt::Display,
) -> fmt::Result {
    write!(
        f,
        ": превышен лимит ресурса {resource}, limit={limit}, observed={observed}"
    )
}

/// Читает, проверяет и нормализует пару по явно заданным путям.
pub fn load_pair<'a>(map: &'a Path, replay: &'a Path) -> Result<LoadedPair, AppError<'a>> {
    let map_bytes = read_limited(map, MAP_LIMIT).map_err(AppError::Read)?;
    let replay_bytes = read_limited(replay, REPLAY_LIMIT).map_err(AppError::Read)?;
    let digest = beatmap_md5(&map_bytes);
    let map_result = osu(&map_bytes).map_err(|source| AppError::Parse {
        path: map,
        source: source.into(),
    })?;
    let replay_result = osr_with_location(&replay_bytes).map_err(|source| AppError::Parse {
        path: replay,
        source,
    })?;
    hash::check_hash(digest, &replay_result.value.meta)
        .map_err(|_| AppError::HashMismatch { map, replay })?;
    let normalized = normalize_with_location(replay_result.value.input).map_err(|source| {
        AppError::Normalize {
            path: replay,
            source,
        }
    })?;
    let mut warnings = map_result.warnings;
    warnings.extend(replay_result.warnings);
    warnings.extend(normalized.warnings);
    Ok(LoadedPair {
        map: map_result.value,
        input: normalized.value,
        meta: replay_result.value.meta,
        expected: replay_result.value.expected,
        warnings,
    })
}

#[derive(Debug)]
pub enum ReadError<'a> {
    Io {
        path: &'a Path,
        source: io::Error,
    },
    Limit {
        path: &'a Path,
        limit: u64,
        observed: u64,
    },
}

impl fmt::Display for ReadError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "IoError [read] {path:?}: {source}"),
            Self::Limit {
                path,
                limit,
                observed,
            } => {
                write!(f, "ResourceLimit [read] {path:?}")?;
                fmt_limit(f, "file_bytes", *limit, observed)
            }
        }
    }
}

impl std::error::Error for ReadError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Limit { .. } => None,
        }
    }
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, ReadError<'_>> {
    let io_error = |source| ReadError::Io { path, source };
    let file = File::open(path).map_err(io_error)?;
    let mut bytes = Vec::new();
    // Лишний байт отличает точную границу от слишком большого файла.
    let observed = file
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)? as u64;
    if observed > limit {
        return Err(ReadError::Limit {
            path,
            limit,
            observed,
        });
    }
    Ok(bytes)
}
