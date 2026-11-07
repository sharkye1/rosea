//! Чтение секции [Events], парсинг перерывов и пропуск остальных событий.

use std::path::Path;

use rosea_core::error::RoseaError;
use rosea_core::model::BreakPeriod;
use rosea_core::parse::osu_breaks;

fn map(events: &str) -> String {
    format!("osu file format v14\n[General]\nMode: 0\n[Events]\n{events}")
}

#[test]
fn reads_breaks_and_skips_other_events() {
    let text = "\u{feff}osu file format v14\r\n[General]\r\nMode: 0\r\n\
        [Events]\r\n\
        // Background and Video events\r\n\
        0,0,\"bg, with, commas.jpg\",0,0\r\n\
        1,500,\"video, with, commas.avi\"\r\n\
        // Break Periods\r\n\
        2, 1000.5 , 2000.5 \r\n\
        Break,3000,4000\r\n\
        \"2\",\"5000\",\"6000\"\r\n\
        2,-500,500\r\n\
        // Storyboard Layer 0 (Background)\r\n\
        Sprite,Pass,Centre,\"sb/sprite, with, commas.png\",320,240\r\n\
         _M,0,1000,2000,100,100\r\n\
        Sample,1234,0,\"sound, with, commas.wav\",100\r\n\
        [TimingPoints]\r\n\
        0,500,4,1,0,100,1,0\r\n";
    assert_eq!(
        osu_breaks(text.as_bytes()),
        Ok(vec![
            BreakPeriod {
                start_time: -500.0,
                end_time: 500.0,
            },
            BreakPeriod {
                start_time: 1000.5,
                end_time: 2000.5,
            },
            BreakPeriod {
                start_time: 3000.0,
                end_time: 4000.0,
            },
            BreakPeriod {
                start_time: 5000.0,
                end_time: 6000.0,
            },
        ])
    );
}

#[test]
fn normalizes_negative_duration_and_signed_zero() {
    let text = map("\
        2, 3000, 1000\n\
        2, -0.0, 500.0\n\
        2, 1000, -0.0\n\
    ");
    assert_eq!(
        osu_breaks(text.as_bytes()),
        Ok(vec![
            BreakPeriod {
                start_time: 0.0,
                end_time: 500.0,
            },
            BreakPeriod {
                start_time: 1000.0,
                end_time: 1000.0,
            },
            BreakPeriod {
                start_time: 3000.0,
                end_time: 3000.0,
            },
        ])
    );
}

#[test]
fn sorts_breaks_by_start_time_and_preserves_order_at_equal_times() {
    let text = map("\
        2, 5000, 6000\n\
        2, 1000, 2000\n\
        2, 1000, 1500\n\
    ");
    assert_eq!(
        osu_breaks(text.as_bytes()),
        Ok(vec![
            BreakPeriod {
                start_time: 1000.0,
                end_time: 2000.0,
            },
            BreakPeriod {
                start_time: 1000.0,
                end_time: 1500.0,
            },
            BreakPeriod {
                start_time: 5000.0,
                end_time: 6000.0,
            },
        ])
    );
}

#[test]
fn accepts_empty_or_missing_events_section() {
    for body in [
        "",
        "[Events]\n\n// comment",
        "[Events]\n0,0,\"bg.jpg\",0,0\nSample,100,0,\"s.wav\",100",
        "[Metadata]\nTitle: Test",
    ] {
        let text = format!("osu file format v14\n[General]\nMode: 0\n{body}");
        assert_eq!(osu_breaks(text.as_bytes()), Ok(vec![]));
    }
}

#[test]
fn rejects_invalid_field_values_with_line_and_field() {
    assert_eq!(
        osu_breaks(map("2,abc,2000").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "startTime",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2,1000,abc").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "endTime",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2,NaN,2000").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "startTime",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2,1000,inf").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "endTime",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2,1000,-inf").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "endTime",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "startTime",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2,1000").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "endTime",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2,1000,2000,extra").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "fields",
            line: 5,
        })
    );
    assert_eq!(
        osu_breaks(map("2,1000,2000,").as_bytes()),
        Err(RoseaError::InvalidBreakField {
            field: "fields",
            line: 5,
        })
    );
}

#[test]
fn checks_header_and_standard_mode() {
    assert_eq!(
        osu_breaks(b"not a valid osu file"),
        Err(RoseaError::InvalidHeader)
    );
    assert_eq!(
        osu_breaks(b"osu file format v13\n[General]\nMode: 0"),
        Err(RoseaError::UnsupportedBeatmapVersion(13))
    );
    assert_eq!(
        osu_breaks(b"osu file format v14\n[General]\nMode: 1\n[Events]\n2,1000,2000"),
        Err(RoseaError::UnsupportedMode(1))
    );
    assert_eq!(
        osu_breaks(&[0xff, 0xff, 0xff]),
        Err(RoseaError::InvalidEncoding)
    );
}

#[test]
fn enforces_break_limit_and_line_limit() {
    let long_line = "2,1000,".to_string() + &"0".repeat(1024 * 1024);
    assert_eq!(
        osu_breaks(map(&long_line).as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "osu_line_bytes",
            limit: 1024 * 1024,
            observed: long_line.len() as u64,
        })
    );

    let mut many_breaks = String::from("osu file format v14\n[General]\nMode: 0\n[Events]\n");
    for i in 0..100_001 {
        many_breaks.push_str(&format!("2,{i},{}\n", i + 10));
    }
    assert_eq!(
        osu_breaks(many_breaks.as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "breaks",
            limit: 100_000,
            observed: 100_001,
        })
    );
}

#[test]
fn reads_real_corpus_maps_with_breaks() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../tests/data");
    if !root.exists() {
        return;
    }

    let test_cases: &[(&str, usize)] = &[
        ("04_errors/galaxy.osu", 7),
        ("02_sliders_geometry/realkaede.osu", 2),
        ("03_spinners/realexit.osu", 1),
        ("02_sliders_geometry/realfallen.osu", 1),
        ("03_spinners/2513983_Dominik393.osu", 1),
        ("02_sliders_geometry/2520609_ampy.osu", 1),
        ("01_micro/dearbrave.osu", 0),
        ("01_micro/tool.osu", 0),
    ];

    for &(relative_path, expected_count) in test_cases {
        let path = root.join(relative_path);
        if !path.exists() {
            continue;
        }
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("Не удалось прочитать {relative_path}: {e}"));
        let breaks = osu_breaks(&bytes)
            .unwrap_or_else(|e| panic!("Ошибка парсинга перерывов в {relative_path}: {e}"));
        assert_eq!(
            breaks.len(),
            expected_count,
            "Неверное число перерывов в {relative_path}"
        );
    }
}
