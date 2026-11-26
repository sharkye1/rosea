use rosea_core::{
    error::{ErrorCode, RoseaError, SourceLocation},
    model::{ObjectId, Warning},
    parse::{beatmap_md5, osu},
};

fn hex(hash: [u8; 16]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn md5_matches_standard_and_block_boundary_vectors() {
    for (input, expected) in [
        (b"".as_slice(), "d41d8cd98f00b204e9800998ecf8427e"),
        (b"abc".as_slice(), "900150983cd24fb0d6963f7d28e17f72"),
        (
            b"message digest".as_slice(),
            "f96b697d7cb7938d525a2f31aaf161d0",
        ),
        (&[b'a'; 55], "ef1772b6dff9a122358552954ad0df65"),
        (&[b'a'; 56], "3b0c8ac703f828b04c6c197006d17218"),
        (&[b'a'; 64], "014842d480b571495a4a0363793f7367"),
        (&vec![b'a'; 1000], "cabe45dcc9ae5b66ba86600cca6b8ba8"),
        (
            b"\xef\xbb\xbfosu file format v14\r\n".as_slice(),
            "f8c6cb69306064d2c6e0fb8dd41547d8",
        ),
    ] {
        assert_eq!(hex(beatmap_md5(input)), expected);
    }
}

#[test]
fn reads_sections_and_reports_unknown_fields() {
    let text = b"osu file format v14\n[General]\nMode:0\nStackLeniency:0.6\nExtra:1\n[Metadata]\nTitle:Test\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\n[Events]\n2,10,20\n[HitObjects]\n256,192,100,1,0\n[Other]\nx:1\n";
    let parsed = osu(text).unwrap();
    assert_eq!(parsed.value.metadata().title, "Test");
    assert_eq!(parsed.value.difficulty().od, 5.0);
    assert_eq!(parsed.value.stack_leniency(), 0.6);
    assert_eq!(parsed.value.object_count(), 1);
    assert_eq!(parsed.value.object_counts().circles, 1);
    assert_eq!(parsed.value.breaks().len(), 1);
    assert_eq!(
        parsed.warnings,
        vec![Warning::UnknownBeatmapFields { count: 2 }]
    );
    assert!(matches!(osu(b"osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\n"), Err(RoseaError::EmptyBeatmap)));
}

#[test]
fn reports_general_field_errors_from_the_public_parser() {
    let suffix =
        "[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:6\n[HitObjects]\n0,0,0,1,0\n";
    for (general, expected) in [
        ("Mode: nope\n", RoseaError::InvalidMode { line: 3 }),
        (
            "Mode: 2\n",
            RoseaError::UnsupportedMode { mode: 2, line: 3 },
        ),
        (
            "Mode: 0\n[General]\nMode: 0\n",
            RoseaError::DuplicateField {
                field: "Mode",
                line: 5,
            },
        ),
        (
            "StackLeniency: 0.5\n[General]\nStackLeniency: 0.5\n",
            RoseaError::DuplicateField {
                field: "StackLeniency",
                line: 5,
            },
        ),
    ] {
        let map = format!("osu file format v14\n[General]\n{general}{suffix}");
        assert_eq!(osu(map.as_bytes()).unwrap_err(), expected);
    }
}

#[test]
fn reports_duplicate_and_required_fields_from_the_public_parser() {
    let prefix = "osu file format v14\n[Difficulty]\nOverallDifficulty:5\nHPDrainRate:6\n[HitObjects]\n0,0,0,1,0\n";
    for (fields, expected) in [
        (
            "[Difficulty]\nCircleSize:4\nCircleSize:5\n",
            RoseaError::DuplicateField {
                field: "CircleSize",
                line: 9,
            },
        ),
        ("", RoseaError::MissingDifficultyField("CircleSize")),
        (
            "[Difficulty]\nCircleSize:\n",
            RoseaError::InvalidDifficultyField {
                field: "CircleSize",
                line: 8,
            },
        ),
        (
            "[Difficulty]\nCircleSize:4\n[Metadata]\nTitle:A\n[Metadata]\nTitle:B\n",
            RoseaError::DuplicateField {
                field: "Title",
                line: 12,
            },
        ),
        (
            "[Difficulty]\nCircleSize:4\n[Colours]\nCombo1:1,2,3\n[Colours]\nCombo1:4,5,6\n",
            RoseaError::DuplicateField {
                field: "Combo1",
                line: 12,
            },
        ),
    ] {
        let map = format!("{prefix}{fields}");
        assert_eq!(osu(map.as_bytes()).unwrap_err(), expected);
    }
}

#[test]
fn exposes_codes_and_source_locations_without_parsing_messages() {
    for (map, code, line, object_id) in [
        (&b"osu file format v14\r\n[General]\r\n\xff"[..], ErrorCode::InvalidEncoding, 3, None),
        (&b"\n\nosu file format v13"[..], ErrorCode::UnsupportedBeatmapVersion, 3, None),
        (&b"\nwrong header"[..], ErrorCode::InvalidHeader, 2, None),
        (&b"osu file format v14\n[General]\nMode:2"[..], ErrorCode::UnsupportedMode, 3, None),
        (&b"osu file format v14\n[Difficulty]\nCircleSize:4\nOverallDifficulty:5\nHPDrainRate:5\n[HitObjects]\n0,0,10,1,0\n0,0,0,128,0"[..], ErrorCode::UnsupportedHitObject, 8, Some(ObjectId(1))),
    ] {
        let error = osu(map).unwrap_err();
        assert_eq!(error.code(), code);
        assert_eq!(error.source_location(), SourceLocation::Line { line, object_id });
    }

    let error =
        osu(b"osu file format v14\n[Difficulty]\nOverallDifficulty:5\nHPDrainRate:5").unwrap_err();
    assert_eq!(error.code(), ErrorCode::MissingDifficultyField);
    assert_eq!(error.source_location(), SourceLocation::None);
    assert_eq!(
        ErrorCode::UnsupportedHitObject.to_string(),
        "UnsupportedHitObject"
    );
}
