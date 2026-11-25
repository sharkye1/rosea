use std::process::{Command, Output};

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
fn accepts_inspect_and_preserves_paths() {
    let out = run(&[
        "inspect",
        "--replay",
        "мой реплей.osr",
        "--map",
        "моя карта.osu",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("InspectUnavailable:"));
    assert!(error.contains("мой реплей.osr"));
    assert!(error.contains("моя карта.osu"));
}
