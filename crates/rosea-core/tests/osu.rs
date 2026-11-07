//! Проверяем заголовок и поля секций на маленьких картах, заданных прямо в тестах.

use rosea_core::error::RoseaError;
use rosea_core::parse::{Difficulty, osu_difficulty, osu_header, osu_mode};

#[test]
fn reads_header() {
    assert_eq!(osu_header(b"osu file format v14\n"), Ok(14));
    assert_eq!(
        osu_header("\u{feff}\r\n\nosu file format v14\r\n".as_bytes()),
        Ok(14)
    );
}

#[test]
fn rejects_bad_header() {
    for text in [
        "",
        "\n",
        "[General]",
        "osu file format vabc",
        "// comment\nosu file format v14",
    ] {
        assert_eq!(osu_header(text.as_bytes()), Err(RoseaError::InvalidHeader));
    }
    assert_eq!(osu_header(b"\xff"), Err(RoseaError::InvalidEncoding));
}

#[test]
fn rejects_other_versions() {
    for version in [13, 128] {
        let text = format!("osu file format v{version}\n");
        assert_eq!(
            osu_header(text.as_bytes()),
            Err(RoseaError::UnsupportedBeatmapVersion(version))
        );
    }
}

#[test]
fn reads_standard_mode_and_defaults_to_zero() {
    for body in [
        "",
        "[General]\nAudioFilename: song.mp3",
        "[General]\n// Mode: 3\n Mode : 0 \n[Metadata]\nMode: 2",
        "[Metadata]\nMode: 1",
    ] {
        let text = format!("\u{feff}osu file format v14\r\n{body}");
        assert_eq!(osu_mode(text.as_bytes()), Ok(0));
    }
}

#[test]
fn rejects_other_modes() {
    for mode in [1, 2, 3] {
        let text = format!("osu file format v14\n[General]\nMode: {mode}");
        assert_eq!(
            osu_mode(text.as_bytes()),
            Err(RoseaError::UnsupportedMode(mode))
        );
    }
}

#[test]
fn rejects_invalid_or_duplicate_mode() {
    for field in ["Mode: abc", "Mode: -1", "Mode: 0.5", "Mode:", "Mode"] {
        let text = format!("osu file format v14\n[General]\n{field}");
        assert_eq!(osu_mode(text.as_bytes()), Err(RoseaError::InvalidMode));
    }
    for body in [
        "Mode: 0\nMode: 0",
        "Mode: 0\n[Metadata]\nTitle: A\n[General]\nMode: 0",
    ] {
        let text = format!("osu file format v14\n[General]\n{body}");
        assert_eq!(osu_mode(text.as_bytes()), Err(RoseaError::DuplicateField));
    }
}

#[test]
fn mode_parser_checks_header() {
    assert_eq!(
        osu_mode(b"[General]\nMode: 0"),
        Err(RoseaError::InvalidHeader)
    );
    assert_eq!(
        osu_mode(b"osu file format v13"),
        Err(RoseaError::UnsupportedBeatmapVersion(13))
    );
    assert_eq!(osu_mode(b"\xff"), Err(RoseaError::InvalidEncoding));
}

#[test]
fn reads_difficulty_fields_in_their_section() {
    let text = "\u{feff}\r\n\nosu file format v14\r\n\
        [General]\r\nCircleSize: invalid\r\nSliderMultiplier: invalid\r\n\
        [Difficulty]\r\n\r\n// CircleSize: 9\r\n\
        HPDrainRate: 6.25\r\n CircleSize : 4.5 \r\n\
        ApproachRate: 9\r\nUnknownField: ignored\r\n\
        OverallDifficulty: 7.75\r\n SliderMultiplier : 1.4 \r\nSliderTickRate: 2\r\n\
        [Metadata]\r\nOverallDifficulty: invalid\r\n";
    assert_eq!(
        osu_difficulty(text.as_bytes()),
        Ok(Difficulty {
            cs: 4.5,
            od: 7.75,
            hp: 6.25,
            ar: 9.0,
            slider_multiplier: Some(1.4),
            slider_tick_rate: Some(2.0),
        })
    );
}

#[test]
fn accepts_difficulty_range_boundaries() {
    for value in [0.0, 10.0] {
        let text = format!(
            "osu file format v14\n[Difficulty]\n\
             CircleSize: {value}\nOverallDifficulty: {value}\nHPDrainRate: {value}"
        );
        assert_eq!(
            osu_difficulty(text.as_bytes()),
            Ok(Difficulty {
                cs: value,
                od: value,
                hp: value,
                ar: value,
                slider_multiplier: None,
                slider_tick_rate: None,
            })
        );
    }
}

#[test]
fn requires_each_difficulty_field() {
    for missing in ["CircleSize", "OverallDifficulty", "HPDrainRate"] {
        let mut text = String::from("osu file format v14\n[Difficulty]\n");
        for field in ["CircleSize", "OverallDifficulty", "HPDrainRate"] {
            if field != missing {
                text.push_str(&format!("{field}: 5\n"));
            }
        }
        text.push_str(&format!("[Metadata]\n{missing}: 5"));
        assert_eq!(
            osu_difficulty(text.as_bytes()),
            Err(RoseaError::MissingDifficultyField(missing))
        );
    }
    for body in ["", "[Difficulty]", "[General]\nCircleSize: 5"] {
        let text = format!("osu file format v14\n{body}");
        assert_eq!(
            osu_difficulty(text.as_bytes()),
            Err(RoseaError::MissingDifficultyField("CircleSize"))
        );
    }
}

#[test]
fn rejects_invalid_difficulty_values_with_field_and_line() {
    for field in [
        "CircleSize",
        "OverallDifficulty",
        "HPDrainRate",
        "ApproachRate",
    ] {
        for value in [
            "",
            "abc",
            "5:6",
            "NaN",
            "inf",
            "-inf",
            "1e999",
            "-0.00000001",
            "10.00000001",
        ] {
            let text = format!("osu file format v14\n[Difficulty]\n\n// comment\n{field}: {value}");
            assert_eq!(
                osu_difficulty(text.as_bytes()),
                Err(RoseaError::InvalidDifficultyField { field, line: 5 }),
                "{field}: {value}"
            );
        }
        let text = format!("osu file format v14\n[Difficulty]\n{field}");
        assert_eq!(
            osu_difficulty(text.as_bytes()),
            Err(RoseaError::InvalidDifficultyField { field, line: 3 })
        );
    }
}

#[test]
fn rejects_duplicate_difficulty_fields() {
    for field in [
        "CircleSize",
        "OverallDifficulty",
        "HPDrainRate",
        "ApproachRate",
        "SliderMultiplier",
        "SliderTickRate",
    ] {
        for separator in ["\n", "\n[Metadata]\nTitle: A\n[Difficulty]\n"] {
            let text = format!(
                "osu file format v14\n[Difficulty]\n\
                 CircleSize: 4\nOverallDifficulty: 5\nHPDrainRate: 6\nApproachRate: 8\n\
                 SliderMultiplier: 1.4\nSliderTickRate: 2{separator}{field}: 7"
            );
            assert_eq!(
                osu_difficulty(text.as_bytes()),
                Err(RoseaError::DuplicateField)
            );
        }
    }
}

#[test]
fn difficulty_parser_checks_header() {
    assert_eq!(
        osu_difficulty(b"[Difficulty]\nCircleSize: 4"),
        Err(RoseaError::InvalidHeader)
    );
    assert_eq!(
        osu_difficulty(b"osu file format v13"),
        Err(RoseaError::UnsupportedBeatmapVersion(13))
    );
    assert_eq!(osu_difficulty(b"\xff"), Err(RoseaError::InvalidEncoding));
}

#[test]
fn reads_ar_boundaries_and_defaults_to_od() {
    for (field, expected) in [
        ("", 7.25),
        ("ApproachRate: 0\n", 0.0),
        ("ApproachRate: 10\n", 10.0),
    ] {
        let text = format!(
            "osu file format v14\n[Difficulty]\n{field}\
             CircleSize: 4\nHPDrainRate: 6\nOverallDifficulty: 7.25\n\
             [Metadata]\nApproachRate: 9"
        );
        assert_eq!(
            osu_difficulty(text.as_bytes()),
            Ok(Difficulty {
                cs: 4.0,
                od: 7.25,
                hp: 6.0,
                ar: expected,
                slider_multiplier: None,
                slider_tick_rate: None,
            })
        );
    }
}

fn map_with_slider_settings(settings: &str, objects: &str) -> String {
    format!(
        "osu file format v14\n[Difficulty]\nCircleSize: 4\nOverallDifficulty: 5\nHPDrainRate: 6\n\
         {settings}\n[HitObjects]\n{objects}"
    )
}

#[test]
fn slider_settings_are_optional_without_sliders() {
    for (settings, multiplier, tick_rate) in [
        ("", None, None),
        ("SliderMultiplier: 1.4", Some(1.4), None),
        ("SliderTickRate: 2", None, Some(2.0)),
    ] {
        // New-combo и цвет круга не должны приниматься за признак слайдера.
        for objects in [
            "",
            "256,192,1000,1,0",
            "256,192,1000,21,0",
            "256,192,1000,8,0,2000",
        ] {
            let text = map_with_slider_settings(settings, objects);
            let difficulty = osu_difficulty(text.as_bytes()).unwrap();
            assert_eq!(difficulty.slider_multiplier, multiplier);
            assert_eq!(difficulty.slider_tick_rate, tick_rate);
        }
    }
}

#[test]
fn reads_positive_slider_settings_without_the_difficulty_upper_bound() {
    for value in [0.000001, 1.4, 20.0, f64::MAX] {
        let settings = format!("SliderMultiplier: {value}\nSliderTickRate: {value}");
        let text = map_with_slider_settings(&settings, "256,192,1000,2,0,L|300:192,1,100");
        let difficulty = osu_difficulty(text.as_bytes()).unwrap();
        assert_eq!(difficulty.slider_multiplier, Some(value));
        assert_eq!(difficulty.slider_tick_rate, Some(value));
    }
}

#[test]
fn rejects_invalid_slider_settings_with_their_own_range_message() {
    for field in ["SliderMultiplier", "SliderTickRate"] {
        for value in ["", "abc", "NaN", "inf", "-inf", "1e999", "0", "-0", "-1"] {
            let text = format!("osu file format v14\n[Difficulty]\n\n// comment\n{field}: {value}");
            let expected = RoseaError::InvalidDifficultyField { field, line: 5 };
            assert_eq!(
                osu_difficulty(text.as_bytes()),
                Err(expected),
                "{field}: {value}"
            );
        }
        let text = format!("osu file format v14\n[Difficulty]\n{field}");
        assert_eq!(
            osu_difficulty(text.as_bytes()),
            Err(RoseaError::InvalidDifficultyField { field, line: 3 })
        );
        let message = RoseaError::InvalidDifficultyField { field, line: 3 }.to_string();
        assert!(message.contains("строго больше 0"));
        assert!(!message.contains("от 0 до 10"));
    }
}

#[test]
fn requires_both_slider_settings_regardless_of_section_order() {
    for flags in [2, 6, 22] {
        for (settings, missing) in [
            ("", "SliderMultiplier"),
            ("SliderTickRate: 2", "SliderMultiplier"),
            ("SliderMultiplier: 1.4", "SliderTickRate"),
        ] {
            let difficulty = format!(
                "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 5\nHPDrainRate: 6\n{settings}"
            );
            let objects = format!("[HitObjects]\n256,192,1000,{flags},0,L|300:192,1,100");
            for sections in [
                format!("{difficulty}\n{objects}"),
                format!("{objects}\n{difficulty}"),
            ] {
                let text = format!("osu file format v14\n{sections}");
                assert_eq!(
                    osu_difficulty(text.as_bytes()),
                    Err(RoseaError::MissingDifficultyField(missing))
                );
            }
        }
    }
}

#[test]
fn reports_an_invalid_object_type_instead_of_assuming_no_sliders() {
    for flags in ["", "abc", "-1", "2.5", "256"] {
        let text = map_with_slider_settings("", &format!("0,0,0,1,0\n256,192,1000,{flags},0"));
        assert_eq!(
            osu_difficulty(text.as_bytes()),
            Err(RoseaError::InvalidHitObjectField {
                field: "type",
                line: 9,
                object_id: rosea_core::model::ObjectId(1)
            })
        );
    }
}

#[test]
fn bounds_the_object_scan_used_for_slider_settings() {
    let mut text = map_with_slider_settings("", &"0,0,0,1,0\n".repeat(100_000));
    assert!(osu_difficulty(text.as_bytes()).is_ok());
    text.push_str("0,0,0,1,0");
    assert_eq!(
        osu_difficulty(text.as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "hit_objects",
            limit: 100_000,
            observed: 100_001
        })
    );
}
