//! Ошибки разбора и лимитов. Вариант ошибки отделён от текста для пользователя.

use std::fmt;

use crate::model::ObjectId;

/// Устойчивый машинный код диагностики.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    UnexpectedEof,
    InvalidLength,
    InvalidStringMarker,
    InvalidReplayString,
    UnsupportedReplayMode,
    UnsupportedMods,
    NoReplayData,
    InvalidLzma,
    InvalidReplayRecord,
    UnsupportedReplayFeature,
    ReplayTimeOverflow,
    BeatmapHashMismatch,
    InvalidEncoding,
    InvalidHeader,
    UnsupportedBeatmapVersion,
    InvalidMode,
    UnsupportedMode,
    InvalidStackLeniency,
    DuplicateField,
    MissingDifficultyField,
    InvalidDifficultyField,
    InvalidMetadataField,
    InvalidColourField,
    EmptyBeatmap,
    InvalidHitObjectField,
    UnsupportedHitObject,
    UnsupportedSliderSyntax,
    InvalidTimingPointField,
    InvalidBreakField,
    InvalidSliderLength,
    InvalidTime,
    TimeWentBackwards,
    ScoreOverflow,
    UnsupportedSimulationObject,
    IncompleteSimulation,
    NumericalError,
    PreprocessError,
    ResourceLimit,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Исходная позиция ошибки. Номера строк начинаются с 1, смещения и индексы — с 0.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SourceLocation {
    None,
    Line {
        line: u32,
        object_id: Option<ObjectId>,
    },
    ByteOffset(u64),
    ReplayRecord(u32),
    Object(ObjectId),
}

// Вызывающий код может различать причины через match, без разбора сообщений Display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoseaError {
    UnexpectedEof {
        offset: usize,
    },
    InvalidLength {
        offset: usize,
    },
    InvalidStringMarker {
        offset: usize,
    },
    InvalidReplayString {
        offset: usize,
    },
    UnsupportedReplayMode {
        mode: u8,
        offset: usize,
    },
    UnsupportedMods {
        mods: u32,
        offset: usize,
    },
    NoReplayData {
        offset: usize,
    },
    InvalidLzma {
        offset: usize,
    },
    InvalidReplayRecord {
        index: u32,
    },
    UnsupportedReplayFeature {
        index: u32,
    },
    ReplayTimeOverflow {
        index: u32,
    },
    BeatmapHashMismatch,
    InvalidEncoding {
        line: usize,
    },
    InvalidHeader {
        line: Option<usize>,
    },
    UnsupportedBeatmapVersion {
        version: u32,
        line: usize,
    },
    InvalidMode {
        line: usize,
    },
    UnsupportedMode {
        mode: u32,
        line: usize,
    },
    InvalidStackLeniency {
        line: usize,
    },
    DuplicateField {
        field: &'static str,
        line: usize,
    },
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
    InvalidTime,
    TimeWentBackwards,
    ScoreOverflow,
    UnsupportedSimulationObject {
        object_id: ObjectId,
    },
    IncompleteSimulation {
        expected: u32,
        actual: u64,
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
    MapResourceLimit {
        resource: &'static str,
        limit: u64,
        observed: u64,
        line: usize,
        object_id: Option<ObjectId>,
    },
}

impl RoseaError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::UnexpectedEof { .. } => ErrorCode::UnexpectedEof,
            Self::InvalidLength { .. } => ErrorCode::InvalidLength,
            Self::InvalidStringMarker { .. } => ErrorCode::InvalidStringMarker,
            Self::InvalidReplayString { .. } => ErrorCode::InvalidReplayString,
            Self::UnsupportedReplayMode { .. } => ErrorCode::UnsupportedReplayMode,
            Self::UnsupportedMods { .. } => ErrorCode::UnsupportedMods,
            Self::NoReplayData { .. } => ErrorCode::NoReplayData,
            Self::InvalidLzma { .. } => ErrorCode::InvalidLzma,
            Self::InvalidReplayRecord { .. } => ErrorCode::InvalidReplayRecord,
            Self::UnsupportedReplayFeature { .. } => ErrorCode::UnsupportedReplayFeature,
            Self::ReplayTimeOverflow { .. } => ErrorCode::ReplayTimeOverflow,
            Self::BeatmapHashMismatch => ErrorCode::BeatmapHashMismatch,
            Self::InvalidEncoding { .. } => ErrorCode::InvalidEncoding,
            Self::InvalidHeader { .. } => ErrorCode::InvalidHeader,
            Self::UnsupportedBeatmapVersion { .. } => ErrorCode::UnsupportedBeatmapVersion,
            Self::InvalidMode { .. } => ErrorCode::InvalidMode,
            Self::UnsupportedMode { .. } => ErrorCode::UnsupportedMode,
            Self::InvalidStackLeniency { .. } => ErrorCode::InvalidStackLeniency,
            Self::DuplicateField { .. } => ErrorCode::DuplicateField,
            Self::MissingDifficultyField(..) => ErrorCode::MissingDifficultyField,
            Self::InvalidDifficultyField { .. } => ErrorCode::InvalidDifficultyField,
            Self::InvalidMetadataField { .. } => ErrorCode::InvalidMetadataField,
            Self::InvalidColourField { .. } => ErrorCode::InvalidColourField,
            Self::EmptyBeatmap => ErrorCode::EmptyBeatmap,
            Self::InvalidHitObjectField { .. } => ErrorCode::InvalidHitObjectField,
            Self::UnsupportedHitObject { .. } => ErrorCode::UnsupportedHitObject,
            Self::UnsupportedSliderSyntax { .. } => ErrorCode::UnsupportedSliderSyntax,
            Self::InvalidTimingPointField { .. } => ErrorCode::InvalidTimingPointField,
            Self::InvalidBreakField { .. } => ErrorCode::InvalidBreakField,
            Self::InvalidSliderLength { .. } => ErrorCode::InvalidSliderLength,
            Self::InvalidTime => ErrorCode::InvalidTime,
            Self::TimeWentBackwards => ErrorCode::TimeWentBackwards,
            Self::ScoreOverflow => ErrorCode::ScoreOverflow,
            Self::UnsupportedSimulationObject { .. } => ErrorCode::UnsupportedSimulationObject,
            Self::IncompleteSimulation { .. } => ErrorCode::IncompleteSimulation,
            Self::NumericalError { .. } => ErrorCode::NumericalError,
            Self::PreprocessError { .. } => ErrorCode::PreprocessError,
            Self::ResourceLimit { .. } | Self::MapResourceLimit { .. } => ErrorCode::ResourceLimit,
        }
    }

    pub fn source_location(&self) -> SourceLocation {
        let (line, object_id) = match *self {
            Self::InvalidEncoding { line }
            | Self::InvalidHeader { line: Some(line) }
            | Self::UnsupportedBeatmapVersion { line, .. }
            | Self::InvalidMode { line }
            | Self::UnsupportedMode { line, .. }
            | Self::InvalidStackLeniency { line }
            | Self::DuplicateField { line, .. }
            | Self::InvalidDifficultyField { line, .. }
            | Self::InvalidMetadataField { line, .. }
            | Self::InvalidColourField { line, .. }
            | Self::InvalidTimingPointField { line, .. }
            | Self::InvalidBreakField { line, .. } => (line, None),
            Self::InvalidHitObjectField {
                line, object_id, ..
            }
            | Self::UnsupportedHitObject {
                line, object_id, ..
            }
            | Self::UnsupportedSliderSyntax { line, object_id }
            | Self::InvalidSliderLength { line, object_id }
            | Self::NumericalError { line, object_id }
            | Self::PreprocessError {
                line, object_id, ..
            } => (line, Some(object_id)),
            Self::MapResourceLimit {
                line, object_id, ..
            } => (line, object_id),
            Self::UnexpectedEof { offset }
            | Self::InvalidLength { offset }
            | Self::InvalidStringMarker { offset }
            | Self::InvalidReplayString { offset }
            | Self::UnsupportedReplayMode { offset, .. }
            | Self::UnsupportedMods { offset, .. }
            | Self::NoReplayData { offset }
            | Self::InvalidLzma { offset } => return SourceLocation::ByteOffset(offset as u64),
            Self::InvalidReplayRecord { index }
            | Self::UnsupportedReplayFeature { index }
            | Self::ReplayTimeOverflow { index } => return SourceLocation::ReplayRecord(index),
            Self::UnsupportedSimulationObject { object_id } => {
                return SourceLocation::Object(object_id);
            }
            Self::InvalidHeader { line: None }
            | Self::MissingDifficultyField(_)
            | Self::EmptyBeatmap
            | Self::BeatmapHashMismatch
            | Self::InvalidTime
            | Self::TimeWentBackwards
            | Self::ScoreOverflow
            | Self::IncompleteSimulation { .. }
            | Self::ResourceLimit { .. } => return SourceLocation::None,
        };
        match u32::try_from(line) {
            Ok(line) => SourceLocation::Line { line, object_id },
            Err(_) => SourceLocation::None,
        }
    }
}

impl fmt::Display for RoseaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { offset } => {
                write!(
                    f,
                    "Недостаточно байтов реплея для чтения поля с позиции {offset}"
                )
            }
            Self::InvalidLength { offset } => write!(f, "Некорректная длина по смещению {offset}"),
            Self::InvalidStringMarker { offset } => {
                write!(f, "Некорректный маркер строки по смещению {offset}")
            }
            Self::InvalidReplayString { offset } => {
                write!(f, "Некорректный UTF-8 строки реплея по смещению {offset}")
            }
            Self::UnsupportedReplayMode { mode, offset } => write!(
                f,
                "Режим реплея {mode} по смещению {offset} не поддерживается"
            ),
            Self::UnsupportedMods { mods, offset } => write!(
                f,
                "Моды реплея {mods} по смещению {offset} не поддерживаются"
            ),
            Self::NoReplayData { offset } => {
                write!(f, "Отсутствуют данные реплея по смещению {offset}")
            }
            Self::InvalidLzma { offset } => {
                write!(f, "Некорректные данные LZMA по смещению {offset}")
            }
            Self::InvalidReplayRecord { index } => write!(f, "Некорректная запись реплея {index}"),
            Self::UnsupportedReplayFeature { index } => {
                write!(f, "Неподдерживаемая дельта в записи реплея {index}")
            }
            Self::ReplayTimeOverflow { index } => {
                write!(f, "Переполнение времени в записи реплея {index}")
            }
            Self::BeatmapHashMismatch => write!(f, "Хеш карты не совпадает с хешем реплея"),
            Self::InvalidEncoding { line } => write!(f, "Строка {line}: карта должна быть в UTF-8"),
            Self::InvalidHeader { line: Some(line) } => {
                write!(f, "Строка {line}: некорректный заголовок .osu")
            }
            Self::InvalidHeader { line: None } => write!(f, "Нет корректного заголовка .osu"),
            Self::UnsupportedBeatmapVersion { version, line } => {
                write!(
                    f,
                    "Строка {line}: версия v{version} не поддерживается, нужна v14"
                )
            }
            Self::InvalidMode { line } => write!(
                f,
                "Строка {line}: в поле Mode должно быть целое неотрицательное число"
            ),
            Self::UnsupportedMode { mode, line } => {
                write!(
                    f,
                    "Строка {line}: режим {mode} не поддерживается, нужен Mode: 0"
                )
            }
            Self::InvalidStackLeniency { line } => {
                write!(
                    f,
                    "Строка {line}: StackLeniency должен быть конечным числом от 0 до 1"
                )
            }
            Self::DuplicateField { field, line } => {
                write!(f, "Строка {line}: поле {field} указано повторно")
            }
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
            Self::InvalidTime => write!(f, "Время должно быть конечным числом"),
            Self::TimeWentBackwards => write!(f, "Время не может идти назад"),
            Self::UnsupportedSimulationObject { object_id } => write!(
                f,
                "Объект {} пока не поддерживается симуляцией",
                object_id.0
            ),
            Self::IncompleteSimulation { expected, actual } => write!(
                f,
                "Симуляция завершена не полностью: объектов {expected}, итоговых оценок {actual}"
            ),
            Self::ScoreOverflow => write!(f, "Переполнение счёта или статистики"),
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
            Self::MapResourceLimit {
                resource,
                limit,
                observed,
                line,
                object_id,
            } => {
                write!(f, "Строка {line}")?;
                if let Some(id) = object_id {
                    write!(f, ", объект {}", id.0)?;
                }
                write!(
                    f,
                    ": превышен лимит {resource}: {observed}, допустимо {limit}"
                )
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
