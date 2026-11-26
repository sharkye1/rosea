use std::{
    fs::{self, OpenOptions},
    io,
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
fn help_reports_broken_stdout_with_exit_code_two() {
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    let out = Command::new(env!("CARGO_BIN_EXE_rosea"))
        .arg("--help")
        .stdout(writer)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("IoError [write]: "), "{error}");
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
fn play_reports_window_availability_before_opening_files() {
    let args = [
        "play",
        "--map",
        "нет карты.osu",
        "--replay",
        "нет реплея.osr",
    ];
    for options in [&[][..], &["--width", "4096", "--height", "2048"]] {
        let args: Vec<_> = args.iter().chain(options).copied().collect();
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        let error = String::from_utf8(out.stderr).unwrap();
        if cfg!(feature = "window") {
            assert!(error.starts_with("PlayUnavailable:"));
        } else {
            assert!(error.starts_with("WindowUnavailable:"));
            assert!(error.contains("feature window"));
        }
    }
}

#[test]
fn rejects_invalid_play_dimensions_as_usage_errors() {
    for options in [
        &["--width", "63"][..],
        &["--height", "4097"],
        &["--height", "１２８"],
        &["--height", "64.0"],
        &["--width", "4096", "--height", "2049"],
        &["--height", "64", "--height", "128"],
    ] {
        let mut args = vec!["play", "--map", "m.osu", "--replay", "r.osr"];
        args.extend_from_slice(options);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{options:?}");
        assert!(out.stdout.is_empty());
        assert!(
            String::from_utf8(out.stderr)
                .unwrap()
                .starts_with("UsageError:"),
            "{options:?}"
        );
    }
}

fn run_image(command: &str, options: &[&str]) -> Output {
    let mut args = vec![
        command,
        "--map",
        "нет карты.osu",
        "--replay",
        "нет реплея.osr",
    ];
    args.extend_from_slice(options);
    run(&args)
}

#[test]
fn image_commands_accept_defaults_and_inclusive_limits() {
    for (command, options, unavailable) in [
        (
            "frame",
            &["--at-ms", "-0.25", "--out", "кадр с пробелом.ppm"][..],
            "FrameUnavailable:",
        ),
        (
            "frame",
            &[
                "--at-ms",
                "-86400000",
                "--out",
                "-",
                "--width",
                "4096",
                "--height",
                "2048",
            ],
            "FrameUnavailable:",
        ),
        (
            "export",
            &["--from-ms", "-2.5", "--to-ms", "-1", "--out", "видео.y4m"],
            "ExportUnavailable:",
        ),
        (
            "export",
            &[
                "--from-ms",
                "-86400000",
                "--to-ms",
                "86400000",
                "--out",
                "-",
                "--fps",
                "1",
            ],
            "ExportUnavailable:",
        ),
        (
            "export",
            &[
                "--width",
                "64",
                "--height",
                "64",
                "--fps",
                "240",
                "--from-ms",
                "0",
                "--to-ms",
                "0.5",
                "--out",
                "-",
            ],
            "ExportUnavailable:",
        ),
    ] {
        let out = run_image(command, options);
        assert_eq!(out.status.code(), Some(2), "{options:?}");
        assert!(out.stdout.is_empty(), "{options:?}");
        let error = String::from_utf8(out.stderr).unwrap();
        assert!(error.starts_with(unavailable), "{options:?}: {error}");
    }
}

#[test]
fn image_commands_reject_invalid_times_fps_and_flag_combinations() {
    for (command, options) in [
        ("frame", &["--out", "-"][..]),
        ("frame", &["--at-ms", "0"]),
        ("frame", &["--at-ms", "0", "--at-ms", "1", "--out", "-"]),
        (
            "frame",
            &["--at-ms", "0", "--out", "-", "--out", "другой.ppm"],
        ),
        (
            "frame",
            &[
                "--at-ms", "0", "--out", "-", "--width", "64", "--width", "65",
            ],
        ),
        ("frame", &["--at-ms", "0", "--out", "-", "--height", "63"]),
        ("frame", &["--at-ms", "0", "--out", "-", "--fps", "60"]),
        ("frame", &["--at-ms", "−1", "--out", "-"]),
        ("frame", &["--at-ms", "NaN", "--out", "-"]),
        ("frame", &["--at-ms", "-inf", "--out", "-"]),
        ("frame", &["--at-ms", "1e999", "--out", "-"]),
        ("export", &["--from-ms", "1", "--out", "-"]),
        ("export", &["--to-ms", "1", "--out", "-"]),
        ("export", &["--from-ms", "-0", "--to-ms", "0", "--out", "-"]),
        (
            "export",
            &["--from-ms", "-1", "--to-ms", "-2", "--out", "-"],
        ),
        (
            "export",
            &[
                "--from-ms",
                "0",
                "--to-ms",
                "1",
                "--to-ms",
                "2",
                "--out",
                "-",
            ],
        ),
        (
            "export",
            &["--from-ms", "0", "--to-ms", "1", "--out", "-", "--fps", "0"],
        ),
        (
            "export",
            &[
                "--from-ms",
                "0",
                "--to-ms",
                "1",
                "--out",
                "-",
                "--fps",
                "60.0",
            ],
        ),
        (
            "export",
            &[
                "--from-ms",
                "0",
                "--to-ms",
                "1",
                "--out",
                "-",
                "--fps",
                "６０",
            ],
        ),
        (
            "export",
            &[
                "--from-ms",
                "0",
                "--to-ms",
                "1",
                "--out",
                "-",
                "--fps",
                "1",
                "--fps",
                "2",
            ],
        ),
    ] {
        let out = run_image(command, options);
        assert_eq!(out.status.code(), Some(2), "{options:?}");
        assert!(out.stdout.is_empty(), "{options:?}");
        let error = String::from_utf8(out.stderr).unwrap();
        assert!(error.starts_with("UsageError:"), "{options:?}: {error}");
    }
}

#[test]
fn image_commands_report_exceeded_limits() {
    for (command, options, limit) in [
        (
            "frame",
            &["--at-ms", "0", "--out", "-", "--width", "4097"][..],
            "image_width, limit=4096, observed=4097",
        ),
        (
            "frame",
            &["--at-ms", "0", "--out", "-", "--height", "4097"],
            "image_height, limit=4096, observed=4097",
        ),
        (
            "frame",
            &[
                "--at-ms", "0", "--out", "-", "--width", "4096", "--height", "2049",
            ],
            "image_pixels, limit=8388608, observed=8392704",
        ),
        (
            "frame",
            &["--at-ms", "-86400000.5", "--out", "-"],
            "absolute_time_ms, limit=86400000, observed=86400000.5",
        ),
        (
            "export",
            &["--from-ms", "0", "--to-ms", "86400001", "--out", "-"],
            "absolute_time_ms, limit=86400000, observed=86400001",
        ),
        (
            "export",
            &[
                "--from-ms",
                "0",
                "--to-ms",
                "1",
                "--out",
                "-",
                "--fps",
                "241",
            ],
            "export_fps, limit=240, observed=241",
        ),
    ] {
        let out = run_image(command, options);
        assert_eq!(out.status.code(), Some(2), "{options:?}");
        assert!(out.stdout.is_empty(), "{options:?}");
        let error = String::from_utf8(out.stderr).unwrap();
        assert!(error.starts_with("ResourceLimit [parse]: "), "{error}");
        assert!(error.contains(limit), "{error}");
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
    assert!(error.starts_with("ResourceLimit [parse]"), "{error}");
    assert!(error.contains("моя карта.osu"), "{error}");
    assert!(error.contains("line=1"), "{error}");
    assert!(
        error.contains("osu_line_bytes, limit=1048576, observed=16777216"),
        "{error}"
    );
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
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("ReplayTrailerNotInterpreted:")
    );

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
    // Две записи с одним временем, неизвестный бит кнопки и seed.
    let compressed = [
        93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 12, 17, 29, 240, 138,
        210, 67, 185, 117, 204, 55, 148, 8, 134, 117, 193, 194, 120, 221, 231, 7, 247, 23, 255,
        255, 90, 28, 0, 0,
    ];
    replay_with_payload(map, &compressed)
}

fn replay_with_payload(map: &[u8], compressed: &[u8]) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut hex = String::new();
    for byte in rosea_core::parse::beatmap_md5(map) {
        write!(hex, "{byte:02X}").unwrap();
    }
    let mut bytes = vec![0];
    bytes.extend_from_slice(&20200427_i32.to_le_bytes());
    bytes.extend_from_slice(&[0x0b, 32]);
    bytes.extend_from_slice(hex.as_bytes());
    bytes.extend_from_slice(&[0; 2 + 12 + 4 + 2 + 1 + 4 + 1 + 8]);
    bytes.extend_from_slice(&i32::try_from(compressed.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(compressed);
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
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let summary = String::from_utf8(out.stdout).unwrap();
    assert!(
        summary.contains("MD5 карты: 21a0dd75772444ef88c39473ce6a3575"),
        "{summary}"
    );
    assert!(summary.contains("Версия клиента (game_version): 20200427"));
    assert!(summary.contains("Объектов: 1"));
    assert!(summary.contains("Нормализованных записей: 2"));
    assert!(summary.contains("Предупреждений: 3"));
    assert!(summary.contains("UnknownBeatmapFields:"));
    assert!(summary.contains("ReplayTrailerNotInterpreted:"));
    assert!(summary.contains("UnknownReplayKeys:"));

    fs::write(&map.0, &MAP_BYTES[3..]).unwrap();
    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.starts_with("BeatmapHashMismatch [parse]"));
    assert!(error.contains("пара карта.osu") && error.contains("пара реплей.osr"));
}

#[test]
fn inspect_counts_each_object_kind_without_preparing_geometry_or_loading_resources() {
    let map_bytes = b"osu file format v14\n[General]\nAudioFilename: absent.mp3\n\
        [Difficulty]\nCircleSize:5\nOverallDifficulty:8\nHPDrainRate:5\nSliderMultiplier:1\nSliderTickRate:1\n\
        [Events]\n0,0,\"absent.jpg\",0,0\nSample,10,0,\"absent.wav\",100\n\
        [HitObjects]\n0,0,10,1,0\n0,0,20,2,0,L|1:1,1,1\n0,0,30,2,0,B|1:1,1,1\n\
        0,0,40,2,0,P|1:1|2:0,1,1\n256,192,50,8,0,100\n";
    let map = TempFile::sized("карта с ресурсами.osu", 0);
    let replay = TempFile::sized("реплей с пробелом.osr", 0);
    fs::write(&map.0, map_bytes).unwrap();
    fs::write(&replay.0, replay_for(map_bytes)).unwrap();

    let out = inspect(&map.0, &replay.0);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stderr.is_empty());
    let summary = String::from_utf8(out.stdout).unwrap();
    assert!(summary.contains("Объектов: 5"), "{summary}");
    assert!(
        summary.contains("Круги: 1, слайдеры L: 1, B: 1, P: 1, спиннеры: 1"),
        "{summary}"
    );

    // Для подготовки той же карты нужна красная timing point.
    let error = rosea_app::run_headless(&map.0, &replay.0).unwrap_err();
    assert!(matches!(error, rosea_app::AppError::Prepare { .. }));
    let message = error.to_string();
    assert!(
        message.starts_with("PreprocessError [prepare]"),
        "{message}"
    );
    assert!(message.contains("карта с ресурсами.osu"), "{message}");
    assert!(message.contains("line=15 object_id=1"), "{message}");
}

#[test]
fn headless_finishes_after_replay_eof_and_ignores_expected_stats() {
    let map_bytes = [MAP_BYTES, b"0,0,1000,1,0\r\n256,192,2000,8,0,4000\r\n"].concat();
    let map = TempFile::sized("карта для прогона.osu", 0);
    let replay = TempFile::sized("ввод для прогона.osr", 0);
    let mut replay_bytes = replay_for(&map_bytes);
    fs::write(&map.0, &map_bytes).unwrap();
    fs::write(&replay.0, &replay_bytes).unwrap();

    // Последняя запись ввода на 10 мс, после неё истекает окно круга и завершается спиннер.
    let first = rosea_app::run_headless(&map.0, &replay.0).unwrap();
    assert_eq!(first.actual.score, 300);
    assert_eq!(first.actual.max_combo, 1);
    assert_eq!(first.actual.current_combo, 0);
    assert_eq!(first.actual.counts.count_300, 1);
    assert_eq!(first.actual.counts.count_100, 0);
    assert_eq!(first.actual.counts.count_50, 0);
    assert_eq!(first.actual.counts.count_miss, 2);
    assert_eq!(first.meta.game_version(), 20200427);
    assert_eq!(first.warnings.len(), 3);

    // Меняем только шесть полей ожиданий в заголовке .osr.
    for offset in [41, 43, 45, 51, 53, 57] {
        replay_bytes[offset] = 7;
    }
    fs::write(&replay.0, &replay_bytes).unwrap();
    let second = rosea_app::run_headless(&map.0, &replay.0).unwrap();
    assert_eq!(second.actual, first.actual);
    assert_eq!(second.warnings, first.warnings);
    assert_eq!(
        second.expected,
        rosea_core::model::ExpectedStats {
            score: 7,
            max_combo: 7,
            count_300: 7,
            count_100: 7,
            count_50: 7,
            count_miss: 7,
        }
    );
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

#[test]
fn inspect_reports_map_line_and_object_id() {
    let map = TempFile::sized("ошибка объекта.osu", 0);
    let replay = TempFile::sized("реплей.osr", 0);
    let mut bytes = MAP_BYTES.to_vec();
    let start = bytes.len() - b"0,0,10,1,0\r\n".len();
    bytes[start] = b'?';
    fs::write(&map.0, bytes).unwrap();

    let error = inspect_error(&map.0, &replay.0);
    assert!(
        error.starts_with("InvalidHitObjectField [parse]"),
        "{error}"
    );
    assert!(error.contains("ошибка объекта.osu"), "{error}");
    assert!(error.contains("line=10 object_id=0"), "{error}");
}

#[test]
fn inspect_reports_binary_field_offset() {
    let map = TempFile::sized("карта.osu", 0);
    let replay = TempFile::sized("усечённый реплей.osr", 0);
    fs::write(&map.0, MAP_BYTES).unwrap();
    fs::write(&replay.0, [0]).unwrap();

    let error = inspect_error(&map.0, &replay.0);
    assert!(error.starts_with("UnexpectedEof [parse]"), "{error}");
    assert!(error.contains("усечённый реплей.osr"), "{error}");
    assert!(error.contains("byte_offset=1"), "{error}");
}

#[test]
fn inspect_reports_replay_limits_with_binary_offset_and_record_index() {
    let map = TempFile::sized("карта.osu", 0);
    let replay = TempFile::sized("лимит реплея.osr", 0);
    fs::write(&map.0, MAP_BYTES).unwrap();

    let mut lzma_header = [0xff; 13];
    lzma_header[0] = 93;
    lzma_header[1..5].copy_from_slice(&67_108_865_u32.to_le_bytes());
    let bytes = replay_with_payload(MAP_BYTES, &lzma_header);
    fs::write(&replay.0, bytes).unwrap();
    let error = inspect_error(&map.0, &replay.0);
    assert!(error.starts_with("ResourceLimit [parse]"), "{error}");
    assert!(error.contains("лимит реплея.osr"), "{error}");
    assert!(error.contains("byte_offset=78"), "{error}");
    assert!(
        error.contains("LZMA dictionary, limit=67108864, observed=67108865"),
        "{error}"
    );

    let compressed =
        include_bytes!("../../rosea-core/tests/fixtures/replay-records-over-limit.lzma");
    fs::write(&replay.0, replay_with_payload(MAP_BYTES, compressed)).unwrap();
    let error = inspect_error(&map.0, &replay.0);
    assert!(error.starts_with("ResourceLimit [parse]"), "{error}");
    assert!(error.contains("лимит реплея.osr"), "{error}");
    assert!(error.contains("record_index=1000000"), "{error}");
    assert!(
        error.contains("replay records, limit=1000000, observed=1000001"),
        "{error}"
    );
}

#[test]
fn inspect_reports_normalization_error_with_original_record_index() {
    let map = TempFile::sized("карта.osu", 0);
    let replay = TempFile::sized("переполнение времени.osr", 0);
    fs::write(&map.0, MAP_BYTES).unwrap();
    // Записи с дельтами i64::MAX и 1 дают переполнение при накоплении времени.
    let compressed = [
        93, 0, 0, 4, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 28, 140, 130, 142, 0, 178, 94,
        135, 151, 226, 193, 115, 250, 91, 126, 250, 192, 241, 83, 131, 53, 9, 22, 22, 83, 11, 66,
        30, 123, 255, 254, 176, 24, 0,
    ];
    fs::write(&replay.0, replay_with_payload(MAP_BYTES, &compressed)).unwrap();

    let error = inspect_error(&map.0, &replay.0);
    assert!(
        error.starts_with("ReplayTimeOverflow [normalize]"),
        "{error}"
    );
    assert!(error.contains("переполнение времени.osr"), "{error}");
    assert!(error.contains("record_index=1"), "{error}");

    // Дельты 0 и 86400001 пересекают предел нормализованного времени.
    let compressed = [
        93, 0, 0, 4, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 31, 31, 4, 88, 56, 16, 64,
        25, 43, 182, 100, 97, 54, 158, 63, 255, 255, 224, 29, 128, 0,
    ];
    fs::write(&replay.0, replay_with_payload(MAP_BYTES, &compressed)).unwrap();
    let error = inspect_error(&map.0, &replay.0);
    assert!(error.starts_with("ResourceLimit [normalize]"), "{error}");
    assert!(error.contains("переполнение времени.osr"), "{error}");
    assert!(error.contains("record_index=1"), "{error}");
    assert!(
        error.contains("replay time ms, limit=86400000, observed=86400001"),
        "{error}"
    );
}

#[test]
fn inspect_reports_when_normalization_removes_all_records() {
    let map = TempFile::sized("карта.osu", 0);
    let replay = TempFile::sized("служебная запись.osr", 0);
    fs::write(&map.0, MAP_BYTES).unwrap();
    // Единственная запись «1|256|-500|0,» удаляется как служебная.
    let compressed = [
        93, 0, 0, 4, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 159, 2, 67, 81, 3, 180, 0,
        85, 86, 145, 45, 200, 215, 80, 123, 35, 255, 255, 217, 81, 0, 0,
    ];
    fs::write(&replay.0, replay_with_payload(MAP_BYTES, &compressed)).unwrap();

    let error = inspect_error(&map.0, &replay.0);
    assert!(error.starts_with("NoReplayData [normalize]"), "{error}");
    assert!(error.contains("служебная запись.osr"), "{error}");
    assert!(!error.contains("byte_offset="), "{error}");
}

fn inspect_error(map: &Path, replay: &Path) -> String {
    let out = inspect(map, replay);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    String::from_utf8(out.stderr).unwrap()
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
