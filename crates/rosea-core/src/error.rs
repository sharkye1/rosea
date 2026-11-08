//! Ошибки разбора и лимитов. Вариант ошибки отделён от текста для пользователя.

use std::fmt;

use crate::model::ObjectId;

// Вызывающий код может различать причины через match, без разбора сообщений Display.
#[derive(Debug, PartialEq, Eq)]
pub enum RoseaError {
    InvalidEncoding,
    InvalidHeader,
    UnsupportedBeatmapVersion(u32),
    InvalidMode,
    UnsupportedMode(u32),
    InvalidStackLeniency {
        line: usize,
    },
    DuplicateField,
    MissingDifficultyField(&'static str),
    InvalidDifficultyField {
        field: &'static str,
        line: usize,
    },
    InvalidMetadataField {
        field: &'static str,
        line: usize,
    },
    InvalidColourField {
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
    UnsupportedSliderSyntax {
        line: usize,
        object_id: ObjectId,
    },
    InvalidTimingPointField {
        field: &'static str,
        line: usize,
    },
    InvalidBreakField {
        field: &'static str,
        line: usize,
    },
    InvalidSliderLength {
        line: usize,
        object_id: ObjectId,
    },
    NumericalError {
        line: usize,
        object_id: ObjectId,
    },
    PreprocessError {
        line: usize,
        object_id: ObjectId,
        message: &'static str,
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
            Self::UnsupportedBeatmapVersion(version) => {
                write!(f, "Версия v{version} не поддерживается, нужна v14")
            }
            Self::InvalidMode => write!(f, "В поле Mode должно быть целое неотрицательное число"),
            Self::UnsupportedMode(mode) => {
                write!(f, "Режим {mode} не поддерживается, нужен Mode: 0")
            }
            Self::InvalidStackLeniency { line } => {
                write!(
                    f,
                    "Строка {line}: StackLeniency должен быть конечным числом от 0 до 1"
                )
            }
            Self::DuplicateField => write!(f, "Поддерживаемое поле указано повторно"),
            Self::MissingDifficultyField(field) => {
                write!(
                    f,
                    "В секции Difficulty отсутствует обязательное поле {field}"
                )
            }
            Self::InvalidDifficultyField { field, line } => {
                let range = if matches!(*field, "SliderMultiplier" | "SliderTickRate") {
                    "конечным числом строго больше 0"
                } else {
                    "конечным числом от 0 до 10"
                };
                write!(f, "Строка {line}: поле {field} должно быть {range}")
            }
            Self::InvalidMetadataField { field, line } => {
                write!(f, "Строка {line}: некорректное поле metadata {field}")
            }
            Self::InvalidColourField { field, line } => {
                write!(f, "Строка {line}: некорректное поле colours {field}")
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
                    "Строка {line}, объект {}: тип объекта или кривая не поддерживается (type={flags})",
                    object_id.0
                )
            }
            Self::UnsupportedSliderSyntax { line, object_id } => {
                write!(
                    f,
                    "Строка {line}, объект {}: синтаксис пути слайдера не поддерживается",
                    object_id.0
                )
            }
            Self::InvalidTimingPointField { field, line } => {
                write!(f, "Строка {line}: некорректное поле timing point {field}")
            }
            Self::InvalidBreakField { field, line } => {
                write!(f, "Строка {line}: некорректное поле перерыва {field}")
            }
            Self::InvalidSliderLength { line, object_id } => {
                write!(
                    f,
                    "Строка {line}, объект {}: длина слайдера должна быть конечной и неотрицательной",
                    object_id.0
                )
            }
            Self::NumericalError { line, object_id } => {
                write!(
                    f,
                    "Строка {line}, объект {}: некорректный численный результат при подготовке или выборке объекта",
                    object_id.0
                )
            }
            Self::PreprocessError {
                line,
                object_id,
                message,
            } => {
                write!(f, "Строка {line}, объект {}: {message}", object_id.0)
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
