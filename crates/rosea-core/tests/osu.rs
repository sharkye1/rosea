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
            Err(RoseaError::UnsupportedVersion(version))
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
        Err(RoseaError::UnsupportedVersion(13))
    );
    assert_eq!(osu_mode(b"\xff"), Err(RoseaError::InvalidEncoding));
}

#[test]
fn reads_difficulty_fields_in_their_section() {
    let text = "\u{feff}\r\n\nosu file format v14\r\n\
        [General]\r\nCircleSize: invalid\r\n\
        [Difficulty]\r\n\r\n// CircleSize: 9\r\n\
        HPDrainRate: 6.25\r\n CircleSize : 4.5 \r\n\
        ApproachRate: 9\r\nUnknownField: ignored\r\n\
        OverallDifficulty: 7.75\r\n\
        [Metadata]\r\nOverallDifficulty: invalid\r\n";
    assert_eq!(
        osu_difficulty(text.as_bytes()),
        Ok(Difficulty {
            cs: 4.5,
            od: 7.75,
            hp: 6.25,
            ar: 9.0,
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
    ] {
        for separator in ["\n", "\n[Metadata]\nTitle: A\n[Difficulty]\n"] {
            let text = format!(
                "osu file format v14\n[Difficulty]\n\
                 CircleSize: 4\nOverallDifficulty: 5\nHPDrainRate: 6\nApproachRate: 8{separator}{field}: 7"
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
        Err(RoseaError::UnsupportedVersion(13))
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
                ar: expected
            })
        );
    }
}
