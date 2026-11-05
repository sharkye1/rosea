#![forbid(unsafe_code)]

pub const PROFILE_ID: &str = "rosea-mvp-v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_id() {
        assert_eq!(PROFILE_ID, "rosea-mvp-v1");
    }
}
