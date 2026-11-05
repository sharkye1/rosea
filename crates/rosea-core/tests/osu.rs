use rosea_core::error::RoseaError;
use rosea_core::parse::osu_header;

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
