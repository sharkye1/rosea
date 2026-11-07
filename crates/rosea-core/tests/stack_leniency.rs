//! Значение StackLeniency, его область действия и проверки исходных данных.

use rosea_core::error::RoseaError;
use rosea_core::parse::osu_stack_leniency;

fn map(body: &str) -> String {
    format!("osu file format v14\n{body}")
}

#[test]
fn reads_finite_f32_values_and_range_boundaries() {
    for (text, expected) in [
        ("0", 0.0_f32),
        ("1", 1.0),
        ("0.7", 0.7),
        ("0.123456789", 0.123_456_79_f32),
        ("5e-1", 0.5),
        // Диапазон проверяется после предусмотренного округления входа к f32.
        ("1.00000001", 1.0),
    ] {
        let text = map(&format!("[General]\nStackLeniency: {text}"));
        assert_eq!(osu_stack_leniency(text.as_bytes()), Ok(expected));
    }
    let text = "\u{feff}osu file format v14\r\n[General]\r\n// StackLeniency: bad\r\n\r\n Mode : 0 \r\n StackLeniency : 0.25 \r\n[Metadata]\r\nStackLeniency: bad";
    assert_eq!(osu_stack_leniency(text.as_bytes()), Ok(0.25));
}

#[test]
fn defaults_when_the_field_or_general_section_is_missing() {
    for body in [
        "",
        "[General]",
        "[General]\nMode: 0\nAudioFilename: missing.mp3",
        "[General]\n// StackLeniency: 0.25",
        "[Difficulty]\nStackLeniency: bad\n[General]\nMode: 0",
        "[General]\n[Metadata]\nStackLeniency: 0.25",
    ] {
        assert_eq!(osu_stack_leniency(map(body).as_bytes()), Ok(0.7));
    }
}

#[test]
fn rejects_invalid_values_with_the_source_line() {
    for value in [
        "", "bad", "NaN", "inf", "-inf", "1e999", "1e99", "-0.0001", "1.0001",
    ] {
        let text = map(&format!("[General]\n// comment\nStackLeniency: {value}"));
        assert_eq!(
            osu_stack_leniency(text.as_bytes()),
            Err(RoseaError::InvalidStackLeniency { line: 4 }),
            "{value}"
        );
    }
    assert_eq!(
        osu_stack_leniency(map("[General]\nStackLeniency").as_bytes()),
        Err(RoseaError::InvalidStackLeniency { line: 3 })
    );
}

#[test]
fn rejects_duplicates_across_repeated_general_sections() {
    for body in [
        "[General]\nStackLeniency: 0\nStackLeniency: 1",
        "[General]\nStackLeniency: 0.7\n[Metadata]\nTitle: A\n[General]\nStackLeniency: 0.7",
    ] {
        assert_eq!(
            osu_stack_leniency(map(body).as_bytes()),
            Err(RoseaError::DuplicateField)
        );
    }
    // Одноимённые ключи в других секциях не считаются объявлением поля General.
    let text = map(
        "[Metadata]\nStackLeniency: bad\n[General]\nStackLeniency: 0.4\n[Difficulty]\nStackLeniency: bad",
    );
    assert_eq!(osu_stack_leniency(text.as_bytes()), Ok(0.4));
}

#[test]
fn checks_the_header_and_standard_mode() {
    assert_eq!(
        osu_stack_leniency(b"\xff"),
        Err(RoseaError::InvalidEncoding)
    );
    assert_eq!(
        osu_stack_leniency(b"[General]\nStackLeniency: 0.7"),
        Err(RoseaError::InvalidHeader)
    );
    assert_eq!(
        osu_stack_leniency(b"osu file format v13"),
        Err(RoseaError::UnsupportedBeatmapVersion(13))
    );
    assert_eq!(
        osu_stack_leniency(map("[General]\nMode: 1\nStackLeniency: 0.7").as_bytes()),
        Err(RoseaError::UnsupportedMode(1))
    );
}

#[test]
fn enforces_the_line_limit_before_trimming_whitespace() {
    const LIMIT: usize = 1024 * 1024;
    let mut line = String::from("StackLeniency:");
    line.push_str(&" ".repeat(LIMIT - line.len() - 1));
    line.push('0');
    assert_eq!(line.len(), LIMIT);
    assert_eq!(
        osu_stack_leniency(map(&format!("[General]\n{line}")).as_bytes()),
        Ok(0.0)
    );
    line.push(' ');
    assert_eq!(
        osu_stack_leniency(map(&format!("[General]\n{line}")).as_bytes()),
        Err(RoseaError::ResourceLimit {
            resource: "osu_line_bytes",
            limit: LIMIT as u64,
            observed: LIMIT as u64 + 1,
        })
    );
}
