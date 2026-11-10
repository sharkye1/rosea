use rosea_core::{
    error::RoseaError,
    model::Warning,
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
