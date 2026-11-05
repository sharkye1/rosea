use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum RoseaError {
    InvalidEncoding,
    InvalidHeader,
    UnsupportedVersion(u32),
}

impl fmt::Display for RoseaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEncoding => write!(f, "Карта должна быть в UTF-8"),
            Self::InvalidHeader => write!(f, "Нет корректного заголовка .osu"),
            Self::UnsupportedVersion(version) => {
                write!(f, "Версия v{version} не поддерживается, нужна v14")
            }
        }
    }
}

impl std::error::Error for RoseaError {}
