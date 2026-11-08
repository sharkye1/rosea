//! Чтение секции `[Metadata]` и поддерживаемых семантических полей карты.

use crate::error::RoseaError;
use crate::model::Metadata;

use super::osu_mode;

const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Читает метаданные карты из секции `[Metadata]`.
///
/// Поддерживает поля: Title, TitleUnicode, Artist, ArtistUnicode, Creator,
/// Version, Source, Tags, BeatmapID и BeatmapSetID.
/// Повторное объявление любого поддерживаемого ключа возвращает ошибку `DuplicateField`.
/// Неизвестные поля и строки комментариев пропускаются.
pub fn osu_metadata(bytes: &[u8]) -> Result<Metadata, RoseaError> {
    osu_mode(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| RoseaError::InvalidEncoding)?;

    let mut in_metadata = false;
    let mut title = None;
    let mut title_unicode = None;
    let mut artist = None;
    let mut artist_unicode = None;
    let mut creator = None;
    let mut version = None;
    let mut source = None;
    let mut tags = None;
    let mut beatmap_id = None;
    let mut beatmap_set_id = None;

    for (index, raw_line) in text.lines().enumerate() {
        if raw_line.len() > MAX_LINE_BYTES {
            return Err(RoseaError::ResourceLimit {
                resource: "osu_line_bytes",
                limit: MAX_LINE_BYTES as u64,
                observed: raw_line.len() as u64,
            });
        }
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') {
            in_metadata = line == "[Metadata]";
            continue;
        }
        if !in_metadata {
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            let key = line.trim();
            if matches!(
                key,
                "Title"
                    | "TitleUnicode"
                    | "Artist"
                    | "ArtistUnicode"
                    | "Creator"
                    | "Version"
                    | "Source"
                    | "Tags"
                    | "BeatmapID"
                    | "BeatmapSetID"
            ) {
                let field = match key {
                    "Title" => "Title",
                    "TitleUnicode" => "TitleUnicode",
                    "Artist" => "Artist",
                    "ArtistUnicode" => "ArtistUnicode",
                    "Creator" => "Creator",
                    "Version" => "Version",
                    "Source" => "Source",
                    "Tags" => "Tags",
                    "BeatmapID" => "BeatmapID",
                    "BeatmapSetID" => "BeatmapSetID",
                    _ => unreachable!(),
                };
                return Err(RoseaError::InvalidMetadataField {
                    field,
                    line: index + 1,
                });
            }
            continue;
        };

        let key = key.trim();
        let value = value.trim();

        match key {
            "Title" => {
                if title.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                title = Some(value.to_owned());
            }
            "TitleUnicode" => {
                if title_unicode.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                title_unicode = Some(value.to_owned());
            }
            "Artist" => {
                if artist.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                artist = Some(value.to_owned());
            }
            "ArtistUnicode" => {
                if artist_unicode.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                artist_unicode = Some(value.to_owned());
            }
            "Creator" => {
                if creator.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                creator = Some(value.to_owned());
            }
            "Version" => {
                if version.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                version = Some(value.to_owned());
            }
            "Source" => {
                if source.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                source = Some(value.to_owned());
            }
            "Tags" => {
                if tags.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                tags = Some(value.to_owned());
            }
            "BeatmapID" => {
                if beatmap_id.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                let id: i32 = value
                    .parse()
                    .map_err(|_| RoseaError::InvalidMetadataField {
                        field: "BeatmapID",
                        line: index + 1,
                    })?;
                beatmap_id = Some(id);
            }
            "BeatmapSetID" => {
                if beatmap_set_id.is_some() {
                    return Err(RoseaError::DuplicateField);
                }
                let id: i32 = value
                    .parse()
                    .map_err(|_| RoseaError::InvalidMetadataField {
                        field: "BeatmapSetID",
                        line: index + 1,
                    })?;
                beatmap_set_id = Some(id);
            }
            _ => continue,
        }
    }

    Ok(Metadata {
        title: title.unwrap_or_default(),
        title_unicode,
        artist: artist.unwrap_or_default(),
        artist_unicode,
        creator: creator.unwrap_or_default(),
        version: version.unwrap_or_default(),
        source: source.unwrap_or_default(),
        tags: tags.unwrap_or_default(),
        beatmap_id,
        beatmap_set_id,
    })
}
