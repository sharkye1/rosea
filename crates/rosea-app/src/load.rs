//! Чтение явно заданных локальных файлов с ограничением размера.

use std::{
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};

pub(crate) const MAP_LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) const REPLAY_LIMIT: u64 = 64 * 1024 * 1024;

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
