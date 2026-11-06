use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum RoseaError {
    InvalidEncoding,
    InvalidHeader,
    UnsupportedVersion(u32),
    InvalidMode,
    UnsupportedMode(u32),
    DuplicateField,
    MissingDifficultyField(&'static str),
    InvalidDifficultyField { field: &'static str, line: usize },
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
        }
    }
}

impl std::error::Error for RoseaError {}
