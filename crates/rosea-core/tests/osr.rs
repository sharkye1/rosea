use rosea_core::{model::Warning, parse::osr, replay::normalize};

fn replay_bytes() -> Vec<u8> {
    let data = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 12, 3, 65, 4, 153,
        91, 83, 160, 122, 219, 176, 37, 27, 45, 1, 127, 255, 225, 190, 128, 0,
    ];
    let mut bytes = vec![0];
    bytes.extend_from_slice(&20131216_i32.to_le_bytes());
    bytes.extend_from_slice(&[0x0b, 1, b'b', 0, 0]);
    for count in 1_u16..=6 {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    bytes.extend_from_slice(&(-42_i32).to_le_bytes());
    bytes.extend_from_slice(&7_u16.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    bytes.extend_from_slice(&(data.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes.extend_from_slice(&[9, 10]);
    bytes
}

#[test]
fn parses_replay_through_public_api() {
    let bytes = replay_bytes();
    let parsed = osr(&bytes).unwrap();
    assert_eq!(parsed.value.meta.game_version(), 20131216);
    assert_eq!(parsed.value.meta.beatmap_hash(), "b");
    assert_eq!(parsed.value.expected.score, -42);
    assert_eq!(parsed.value.expected.count_miss, 6);
    assert_eq!(parsed.value.meta.count_geki, 4);
    assert_eq!(parsed.value.input.records()[0].delta_ms, 10);
    assert_eq!(parsed.value.meta.trailer, [9, 10]);
    assert_eq!(parsed.warnings, vec![Warning::ReplayTrailerNotInterpreted]);
    let timeline = normalize(parsed.value.input).unwrap();
    assert_eq!(timeline.value.record_count(), 1);
    assert_eq!(timeline.value.records()[0].time_ms, 10);
    assert!(timeline.warnings.is_empty());
    assert!(osr(&bytes[..bytes.len() - 3]).is_err());
}

#[test]
fn client_version_changes_only_metadata_in_public_replay_pipeline() {
    let reference_bytes = replay_bytes();
    let reference = osr(&reference_bytes).unwrap();
    let reference_seed = reference.value.input.seed();
    let expected = normalize(reference.value.input).unwrap();

    for version in [0, 1, 20131216, i32::MAX] {
        let mut bytes = reference_bytes.clone();
        bytes[1..5].copy_from_slice(&version.to_le_bytes());

        let parsed = osr(&bytes).unwrap();
        assert_eq!(parsed.value.meta.game_version(), version);
        assert_eq!(parsed.value.expected, reference.value.expected);
        assert_eq!(parsed.value.input.seed(), reference_seed);
        assert_eq!(parsed.warnings, reference.warnings);

        let meta = &parsed.value.meta;
        let reference_meta = &reference.value.meta;
        assert_eq!(meta.mode, reference_meta.mode);
        assert_eq!(meta.beatmap_hash(), reference_meta.beatmap_hash());
        assert_eq!(meta.player, reference_meta.player);
        assert_eq!(meta.replay_hash, reference_meta.replay_hash);
        assert_eq!(meta.count_geki, reference_meta.count_geki);
        assert_eq!(meta.count_katu, reference_meta.count_katu);
        assert_eq!(meta.perfect, reference_meta.perfect);
        assert_eq!(meta.mods, reference_meta.mods);
        assert_eq!(meta.life_graph, reference_meta.life_graph);
        assert_eq!(meta.timestamp, reference_meta.timestamp);
        assert_eq!(meta.trailer, reference_meta.trailer);

        let normalized = normalize(parsed.value.input).unwrap();
        assert_eq!(normalized.value, expected.value, "версия {version}");
        assert_eq!(normalized.warnings, expected.warnings, "версия {version}");
    }
}
