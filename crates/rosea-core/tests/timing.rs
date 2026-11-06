//! Проверяем поля точек темпа, порядок равновременных записей и диагностику ошибок.

use rosea_core::error::RoseaError;
use rosea_core::model::TimingPoint;
use rosea_core::parse::osu_timing_points;

fn map(points: &str) -> String {
    format!("osu file format v14\n[TimingPoints]\n{points}")
}

#[test]
fn reads_all_fields_and_keeps_source_order_at_equal_times() {
    let text = "\u{feff}osu file format v14\r\n[General]\r\nMode: 0\r\n\
        [TimingPoints]\r\n1000.5,500.25,3,2,7,80,1,9\r\n\r\n// comment\r\n\
        -10.25,600,4,0,0,0,1,0\r\n[Metadata]\r\nignored\r\n\
        [TimingPoints]\r\n 1000.5 , 333.33 , 7 , 3 , 1 , 100 , 1 , 1 \r\n";
    assert_eq!(
        osu_timing_points(text.as_bytes()),
        Ok(vec![
            TimingPoint {
                line: 8,
                time: -10.25,
                beat_length: 600.0,
                meter: 4,
                sample_set: 0,
                sample_index: 0,
                volume: 0,
                effects: 0
            },
            TimingPoint {
                line: 5,
                time: 1000.5,
                beat_length: 500.25,
                meter: 3,
                sample_set: 2,
                sample_index: 7,
                volume: 80,
                effects: 9
            },
            TimingPoint {
                line: 12,
                time: 1000.5,
                beat_length: 333.33,
                meter: 7,
                sample_set: 3,
                sample_index: 1,
                volume: 100,
                effects: 1
            },
        ])
    );
}

#[test]
fn accepts_empty_or_missing_timing_section() {
    for body in [
        "",
        "[TimingPoints]\n\n// comment",
        "[Metadata]\n0,500,4,1,0,100,1,0",
    ] {
        let text = format!("osu file format v14\n{body}");
        assert_eq!(osu_timing_points(text.as_bytes()), Ok(vec![]));
    }
}

#[test]
fn canonicalizes_zero_and_preserves_all_effect_bits() {
    let text = map("-0,500,1,1,4294967295,0,1,4294967295\n0,500,4,1,0,100,1,8");
    let points = osu_timing_points(text.as_bytes()).unwrap();
    assert_eq!(points[0].line, 3);
    assert_eq!(points[1].line, 4);
    assert_eq!(points[0].time.to_bits(), 0.0_f64.to_bits());
    assert_eq!(points[1].time.to_bits(), 0.0_f64.to_bits());
    assert_eq!(points[0].meter, 1);
    assert_eq!(points[0].sample_set, 1);
    assert_eq!(points[0].sample_index, u32::MAX);
    assert_eq!(points[0].effects, u32::MAX);
    assert_eq!(points[1].effects, 8);
}

#[test]
fn rejects_invalid_field_values_with_line_and_field() {
    for (index, field, values) in [
        (0, "time", vec!["", "abc", "NaN", "inf", "-inf", "1e999"]),
        (
            1,
            "beatLength",
            vec!["", "abc", "NaN", "inf", "-inf", "1e999", "0", "-0", "-500"],
        ),
        (2, "meter", vec!["", "abc", "0", "-1", "1.5", "4294967296"]),
        (3, "sampleSet", vec!["", "abc", "-1", "1.5", "4", "256"]),
        (4, "sampleIndex", vec!["", "abc", "-1", "1.5", "4294967296"]),
        (5, "volume", vec!["", "abc", "-1", "1.5", "101", "256"]),
        (6, "uninherited", vec!["", "abc", "-1", "1.5", "2", "256"]),
        (7, "effects", vec!["", "abc", "-1", "1.5", "4294967296"]),
    ] {
        for value in values {
            let mut fields = ["0", "500", "4", "1", "0", "100", "1", "0"];
            fields[index] = value;
            let text = map(&format!("0,500,4,1,0,100,1,0\n{}", fields.join(",")));
            assert_eq!(
                osu_timing_points(text.as_bytes()),
                Err(RoseaError::InvalidTimingPointField { field, line: 4 }),
                "{field}: {value}"
            );
        }
    }
}

#[test]
fn requires_exactly_eight_fields() {
    let fields = ["0", "500", "4", "1", "0", "100", "1", "0"];
    for (count, missing) in [
        (1, "beatLength"),
        (2, "meter"),
        (3, "sampleSet"),
        (4, "sampleIndex"),
        (5, "volume"),
        (6, "uninherited"),
        (7, "effects"),
    ] {
        let text = map(&fields[..count].join(","));
        assert_eq!(
            osu_timing_points(text.as_bytes()),
            Err(RoseaError::InvalidTimingPointField {
                field: missing,
                line: 3
            })
        );
    }
    for extra in [",", ",extra"] {
        let text = map(&format!("{}{extra}", fields.join(",")));
        assert_eq!(
            osu_timing_points(text.as_bytes()),
            Err(RoseaError::InvalidTimingPointField {
                field: "fields",
                line: 3
            })
        );
    }
}

#[test]
fn rejects_inherited_points_instead_of_skipping_them() {
    let text = map("0,500,4,1,0,100,1,0\n1000,-50,4,1,0,80,0,1");
    assert_eq!(
        osu_timing_points(text.as_bytes()),
        Err(RoseaError::UnsupportedTimingPoint { line: 4 })
    );
}

#[test]
fn checks_header_and_standard_mode() {
    assert_eq!(osu_timing_points(b"\xff"), Err(RoseaError::InvalidEncoding));
    assert_eq!(
        osu_timing_points(b"[TimingPoints]"),
        Err(RoseaError::InvalidHeader)
    );
    assert_eq!(
        osu_timing_points(b"osu file format v13"),
        Err(RoseaError::UnsupportedVersion(13))
    );
    let text = "osu file format v14\n[General]\nMode: 1\n[TimingPoints]\n0,500,4,1,0,100,1,0";
    assert_eq!(
        osu_timing_points(text.as_bytes()),
        Err(RoseaError::UnsupportedMode(1))
    );
}

#[test]
fn enforces_timing_point_and_line_limits() {
    let mut text = map(&"0,500,4,1,0,100,1,0\n".repeat(100_000));
    assert_eq!(osu_timing_points(text.as_bytes()).unwrap().len(), 100_000);
    text.push_str("0,500,4,1,0,100,1,0");
    assert_eq!(
        osu_timing_points(text.as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "timing_points",
            limit: 100_000,
            observed: 100_001
        })
    );

    let prefix = "0,500,4,1,0,100,1,";
    // Дополняем effects нулями: поле остаётся корректным, пока проверяем длину строки.
    let mut point = prefix.to_owned();
    point.push_str(&"0".repeat(1024 * 1024 - prefix.len()));
    assert_eq!(osu_timing_points(map(&point).as_bytes()).unwrap().len(), 1);
    point.push('0');
    assert_eq!(
        osu_timing_points(map(&point).as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "osu_line_bytes",
            limit: 1_048_576,
            observed: 1_048_577
        })
    );
}
