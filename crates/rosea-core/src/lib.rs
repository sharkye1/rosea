//! Ядро Rosea: получает байты от приложения и возвращает разобранные данные.

#![forbid(unsafe_code)]

pub mod error;
pub mod model;
pub mod parse;
pub mod prepare;
pub mod replay;
pub mod sim;

pub const PROFILE_ID: &str = "rosea-mvp-v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_id() {
        assert_eq!(PROFILE_ID, "rosea-mvp-v1");
    }
}
