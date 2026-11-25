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
    let out = inspect(&map.0, Path::new("мой реплей.osr"));
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("InspectUnavailable:"));
    assert!(error.contains("мой реплей.osr"));
    assert!(error.contains("моя карта.osu"));
    assert!(error.contains("16777216"));
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
