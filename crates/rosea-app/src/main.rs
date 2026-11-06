//! Точка входа приложения. Пока выводит только название профиля; CLI ещё не реализован.

#![forbid(unsafe_code)]

fn main() {
    println!("Rosea ({})", rosea_core::PROFILE_ID);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_smoke() {
        assert_eq!(rosea_core::PROFILE_ID, "rosea-mvp-v1");
    }
}
