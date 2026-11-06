//! Ошибки разбора и лимитов. Вариант ошибки отделён от текста для пользователя.

use std::fmt;

use crate::model::ObjectId;

// Вызывающий код может различать причины через match, без разбора сообщений Display.
#[derive(Debug, PartialEq, Eq)]
pub enum RoseaError {
    InvalidEncoding,
    InvalidHeader,
    UnsupportedVersion(u32),
    InvalidMode,
    UnsupportedMode(u32),
    DuplicateField,
    MissingDifficultyField(&'static str),
    InvalidDifficultyField {
        field: &'static str,
        line: usize,
    },
    EmptyBeatmap,
    InvalidHitObjectField {
        field: &'static str,
        line: usize,
        object_id: ObjectId,
    },
    UnsupportedHitObject {
        line: usize,
        object_id: ObjectId,
        flags: u8,
    },
    InvalidTimingPointField {
        field: &'static str,
        line: usize,
    },
    ResourceLimit {
        resource: &'static str,
        limit: u64,
        observed: u64,
    },
}

impl fmt::Display for RoseaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEncoding => write!(f, "Карта должна быть в UTF-8"),
            Self::InvalidHeader => write!(f, "Нет корректного заголовка .osu"),
            Self::UnsupportedVersion(version) => {
                write!(f, "Версия v{version} не поддерживается, нужна v14")
            }
            Self::InvalidMode => write!(f, "В поле Mode должно быть целое неотрицательное число"),
            Self::UnsupportedMode(mode) => {
                write!(f, "Режим {mode} не поддерживается, нужен Mode: 0")
            }
            Self::DuplicateField => write!(f, "Поддерживаемое поле указано повторно"),
            Self::MissingDifficultyField(field) => {
                write!(
                    f,
                    "В секции Difficulty отсутствует обязательное поле {field}"
                )
            }
            Self::InvalidDifficultyField { field, line } => {
                write!(
                    f,
                    "Строка {line}: поле {field} должно быть конечным числом от 0 до 10"
                )
            }
            Self::EmptyBeatmap => write!(f, "В карте нет объектов HitObjects"),
            Self::InvalidHitObjectField {
                field,
                line,
                object_id,
            } => {
                write!(
                    f,
                    "Строка {line}, объект {}: некорректное поле {field}",
                    object_id.0
                )
            }
            Self::UnsupportedHitObject {
                line,
                object_id,
                flags,
            } => {
                write!(
                    f,
                    "Строка {line}, объект {}: тип {flags} не поддерживается, пока доступны только круги",
                    object_id.0
                )
            }
            Self::InvalidTimingPointField { field, line } => {
                write!(f, "Строка {line}: некорректное поле timing point {field}")
            }
            Self::ResourceLimit {
                resource,
                limit,
                observed,
            } => {
                write!(
                    f,
                    "Превышен лимит {resource}: {observed}, допустимо {limit}"
                )
            }
        }
    }
}

impl std::error::Error for RoseaError {}
