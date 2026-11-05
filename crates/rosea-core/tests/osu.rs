use rosea_core::error::RoseaError;
use rosea_core::parse::{osu_header, osu_mode};

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
