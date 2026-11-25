use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{self, Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rosea"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_succeeds_on_stdout() {
    for args in [&["--help"][..], &["inspect", "--help"]] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(0));
        assert!(
            String::from_utf8(out.stdout)
                .unwrap()
                .contains("inspect --map")
        );
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn rejects_invalid_command_lines() {
    for args in [
        &[][..],
        &["compare"],
        &["inspect"],
        &["inspect", "--replay", "r.osr"],
        &["inspect", "--map", "m.osu"],
        &["inspect", "--map", "--replay", "r.osr"],
        &["inspect", "--map", "m.osu", "--replay"],
        &[
            "inspect", "--map", "m.osu", "--map", "m2.osu", "--replay", "r.osr",
        ],
        &[
            "inspect", "--replay", "r.osr", "--replay", "r2.osr", "--map", "m.osu",
        ],
        &["inspect", "--map", "m.osu", "--replay", "r.osr", "--json"],
        &["inspect", "--help", "--map", "m.osu"],
        &["--help", "inspect"],
    ] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8(out.stderr)
                .unwrap()
                .starts_with("UsageError:"),
            "{args:?}"
        );
    }
}

#[test]
fn inspection_reads_a_map_at_the_limit_and_preserves_paths() {
    let map = TempFile::sized("моя карта.osu", 16 * 1024 * 1024);
    let replay = TempFile::sized("мой реплей.osr", 0);
    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("MapParseError [parse]"));
    assert!(error.contains("моя карта.osu"));
}

#[test]
fn rejects_a_map_one_byte_over_the_limit() {
    let map = TempFile::sized("большая карта.osu", 16 * 1024 * 1024 + 1);
    let out = inspect(&map.0, Path::new("мой реплей.osr"));
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("ResourceLimit [read]"));
    assert!(error.contains("большая карта.osu"));
    assert!(error.contains("limit=16777216"));
    assert!(error.contains("observed=16777217"));
}

#[test]
fn enforces_replay_file_limit_including_trailer() {
    let map = TempFile::sized("карта.osu", 0);
    fs::write(&map.0, MAP_BYTES).unwrap();
    let replay = TempFile::sized("реплей с хвостом.osr", 0);
    fs::write(&replay.0, replay_for(MAP_BYTES)).unwrap();
    let file = OpenOptions::new().write(true).open(&replay.0).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    drop(file);

    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("InspectUnavailable:"));

    OpenOptions::new()
        .write(true)
        .open(&replay.0)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("ResourceLimit [read]"));
    assert!(error.contains("реплей с хвостом.osr"));
    assert!(error.contains("limit=67108864"));
    assert!(error.contains("observed=67108865"));
}

const MAP_BYTES: &[u8] = b"\xef\xbb\xbfosu file format v14\r\n[General]\r\nAudioFilename: missing.mp3\r\nUnknownField: 1\r\n[Difficulty]\r\nCircleSize:5\r\nOverallDifficulty:8\r\nHPDrainRate:5\r\n[HitObjects]\r\n0,0,10,1,0\r\n";

fn replay_for(map: &[u8]) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut hex = String::new();
    for byte in rosea_core::parse::beatmap_md5(map) {
        write!(hex, "{byte:02X}").unwrap();
    }
    // Две записи с одним временем, неизвестный бит кнопки и seed.
    let compressed = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 12, 17, 29, 240, 138,
        210, 67, 185, 117, 204, 55, 148, 8, 134, 117, 193, 194, 120, 221, 231, 7, 247, 23, 255,
        255, 90, 28, 0, 0,
    ];
    let mut bytes = vec![0];
    bytes.extend_from_slice(&20200427_i32.to_le_bytes());
    bytes.extend_from_slice(&[0x0b, 32]);
    bytes.extend_from_slice(hex.as_bytes());
    bytes.extend_from_slice(&[0; 2 + 12 + 4 + 2 + 1 + 4 + 1 + 8]);
    bytes.extend_from_slice(&i32::try_from(compressed.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&compressed);
    bytes.push(9);
    bytes
}

#[test]
fn inspect_verifies_raw_bytes_and_preserves_all_loader_warnings() {
    let map = TempFile::sized("пара карта.osu", 0);
    let replay = TempFile::sized("пара реплей.osr", 0);
    fs::write(&map.0, MAP_BYTES).unwrap();
    fs::write(&replay.0, replay_for(MAP_BYTES)).unwrap();
    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("InspectUnavailable: пара проверена"));
    assert!(error.contains("объектов 1, записей 2, версия 20200427, предупреждений 3"));

    fs::write(&map.0, &MAP_BYTES[3..]).unwrap();
    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("BeatmapHashMismatch [parse]"));
    assert!(error.contains("пара карта.osu") && error.contains("пара реплей.osr"));
}

#[test]
fn reports_missing_replay_with_path_and_io_cause() {
    let map = TempFile::sized("карта.osu", 0);
    let replay = TempFile::sized("отсутствующий реплей.osr", 0);
    fs::remove_file(&replay.0).unwrap();
    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    let (context, cause) = error.trim().split_once(": ").unwrap();
    assert!(context.starts_with("IoError [read]"));
    assert!(context.contains("отсутствующий реплей.osr"));
    assert!(!cause.is_empty());
}

fn inspect(map: &Path, replay: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rosea"))
        .args(["inspect", "--replay"])
        .arg(replay)
        .arg("--map")
        .arg(map)
        .output()
        .unwrap()
}

struct TempFile(PathBuf);

impl TempFile {
    fn sized(name: &str, len: u64) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rosea-{}-{index}-{name}", process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let fixture = Self(path);
        file.set_len(len).unwrap();
        fixture
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
