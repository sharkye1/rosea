//! Чтение явно заданных локальных файлов с ограничением размера.

use rosea_core::{
    error::RoseaError,
    model::{Beatmap, ExpectedStats, ReplayMeta, Warning},
    parse::{beatmap_md5, osr, osu},
    replay::{ReplayTimeline, normalize},
};
use std::{
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};

use crate::hash;

pub(crate) const MAP_LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) const REPLAY_LIMIT: u64 = 64 * 1024 * 1024;

pub(crate) struct LoadedPair {
    pub(crate) map: Beatmap,
    pub(crate) input: ReplayTimeline,
    pub(crate) meta: ReplayMeta,
    pub(crate) expected: ExpectedStats,
    pub(crate) warnings: Vec<Warning>,
}

pub(crate) enum LoadError<'a> {
    Read(ReadError<'a>),
    ParseMap { path: &'a Path, source: RoseaError },
    ParseReplay { path: &'a Path, source: RoseaError },
    HashMismatch { map: &'a Path, replay: &'a Path },
    Normalize { path: &'a Path, source: RoseaError },
}

impl fmt::Display for LoadError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(f),
            Self::ParseMap { path, source } => {
                write!(f, "MapParseError [parse] {path:?}: {source}")
            }
            Self::ParseReplay { path, source } => {
                write!(f, "ReplayParseError [parse] {path:?}: {source}")
            }
            Self::HashMismatch { map, replay } => write!(
                f,
                "BeatmapHashMismatch [parse]: карта {map:?} не соответствует реплею {replay:?}"
            ),
            Self::Normalize { path, source } => {
                write!(f, "ReplayNormalizeError [normalize] {path:?}: {source}")
            }
        }
    }
}

pub(crate) fn load_pair<'a>(map: &'a Path, replay: &'a Path) -> Result<LoadedPair, LoadError<'a>> {
    let map_bytes = read_limited(map, MAP_LIMIT).map_err(LoadError::Read)?;
    let replay_bytes = read_limited(replay, REPLAY_LIMIT).map_err(LoadError::Read)?;
    let digest = beatmap_md5(&map_bytes);
    let map_result = osu(&map_bytes).map_err(|source| LoadError::ParseMap { path: map, source })?;
    let replay_result = osr(&replay_bytes).map_err(|source| LoadError::ParseReplay {
        path: replay,
        source,
    })?;
    hash::check_hash(digest, &replay_result.value.meta)
        .map_err(|_| LoadError::HashMismatch { map, replay })?;
    let normalized =
        normalize(replay_result.value.input).map_err(|source| LoadError::Normalize {
            path: replay,
            source,
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

pub(crate) enum ReadError<'a> {
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
            } => write!(
                f,
                "ResourceLimit [read] {path:?}: file_bytes, limit={limit}, observed={observed}"
            ),
        }
    }
}

pub(crate) fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, ReadError<'_>> {
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
