use rosea_core::{error::RoseaError, model::TimeMs};

#[test]
fn preserves_finite_times_and_canonicalizes_zero() {
    for time in [-f64::MAX, -1000.25, 1000.25, f64::MAX] {
        assert_eq!(TimeMs::new(time).unwrap().get().to_bits(), time.to_bits());
    }
    for time in [-0.0_f64, 0.0] {
        assert_eq!(
            TimeMs::new(time).unwrap().get().to_bits(),
            0.0_f64.to_bits()
        );
    }
}

#[test]
fn rejects_non_finite_times() {
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(TimeMs::new(time), Err(RoseaError::InvalidTime));
    }
}
