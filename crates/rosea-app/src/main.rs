//! Точка входа приложения Rosea. Поддерживает разбор карт и реплеев с выводом в JSON.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rosea_app::check_beatmap_hash;
use rosea_core::error::RoseaError;
use rosea_core::model::{
    Beatmap, BreakPeriod, Colours, CurveKind, HitObjectKind, Metadata, ReplayChange, Rgb, Warning,
};
use rosea_core::parse::{beatmap_md5, osr, osu};
use rosea_core::replay::{ReplayTimeline, normalize};

const MAX_OSU_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_OSR_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub const ROSEA_COMMIT: &str = match option_env!("ROSEA_COMMIT") {
    Some(val) => val,
    None => "dev",
};

enum CliAction {
    Version,
    Help,
    Parse {
        map_path: PathBuf,
        replay_path: Option<PathBuf>,
        json: bool,
    },
}

fn parse_cli_args<I>(raw_args: I) -> Result<CliAction, (String, bool)>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = raw_args.into_iter().collect();
    if args.len() <= 1 {
        return Ok(CliAction::Version);
    }

    let mut map_path: Option<PathBuf> = None;
    let mut replay_path: Option<PathBuf> = None;
    let mut json_output = false;
    let mut parse_err: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.to_str() {
            Some("-v" | "--version" | "version") if parse_err.is_none() => {
                return Ok(CliAction::Version);
            }
            Some("-h" | "--help" | "help") if parse_err.is_none() => {
                return Ok(CliAction::Help);
            }
            Some("--json") => {
                json_output = true;
            }
            Some("--map") => {
                i += 1;
                if i >= args.len() {
                    if parse_err.is_none() {
                        parse_err = Some("Для флага --map не указан путь к файлу .osu".to_string());
                    }
                    break;
                }
                if map_path.is_none() {
                    map_path = Some(PathBuf::from(&args[i]));
                } else if parse_err.is_none() {
                    parse_err = Some("Флаг --map указан повторно".to_string());
                }
            }
            Some("--replay") => {
                i += 1;
                if i >= args.len() {
                    if parse_err.is_none() {
                        parse_err =
                            Some("Для флага --replay не указан путь к файлу .osr".to_string());
                    }
                    break;
                }
                if replay_path.is_none() {
                    replay_path = Some(PathBuf::from(&args[i]));
                } else if parse_err.is_none() {
                    parse_err = Some("Флаг --replay указан повторно".to_string());
                }
            }
            Some("parse" | "dump") if map_path.is_none() => {
                // Необязательная подкоманда.
            }
            Some(s) if s.starts_with('-') => {
                if parse_err.is_none() {
                    parse_err = Some(format!("Неизвестный флаг: {s}"));
                }
            }
            _ => {
                if map_path.is_none() {
                    map_path = Some(PathBuf::from(arg));
                } else if parse_err.is_none() {
                    parse_err = Some(format!(
                        "Лишний позиционный аргумент: {}",
                        arg.to_string_lossy()
                    ));
                }
            }
        }
        i += 1;
    }

    if let Some(err_msg) = parse_err {
        return Err((err_msg, json_output));
    }

    let Some(file_path) = map_path else {
        return Err((
            "Не указан файл карты (.osu). Запустите с --help для справки.".to_string(),
            json_output,
        ));
    };

    Ok(CliAction::Parse {
        map_path: file_path,
        replay_path,
        json: json_output,
    })
}

fn main() -> ExitCode {
    match parse_cli_args(std::env::args_os()) {
        Ok(CliAction::Version) => {
            println!("Rosea ({}) @ {}", rosea_core::PROFILE_ID, ROSEA_COMMIT);
            ExitCode::SUCCESS
        }
        Ok(CliAction::Help) => {
            print_help();
            ExitCode::SUCCESS
        }
        Ok(CliAction::Parse {
            map_path,
            replay_path,
            json,
        }) => match run_parse(&map_path, replay_path.as_deref(), json) {
            Ok(output) => {
                println!("{output}");
                ExitCode::SUCCESS
            }
            Err((err_code, err_msg, replay_json)) => {
                if json {
                    let replay_part = replay_json.as_deref().unwrap_or("null");
                    println!(
                        r#"{{"status":"error","profile":"{}","commit":"{}","error_code":"{}","error":"{}","replay":{}}}"#,
                        rosea_core::PROFILE_ID,
                        ROSEA_COMMIT,
                        escape_json(err_code),
                        escape_json(&err_msg),
                        replay_part
                    );
                } else {
                    eprintln!("rosea error [{err_code}]: {err_msg}");
                }
                ExitCode::from(1)
            }
        },
        Err((err_msg, json_output)) => {
            if json_output {
                println!(
                    r#"{{"status":"error","profile":"{}","commit":"{}","error_code":"UsageError","error":"{}"}}"#,
                    rosea_core::PROFILE_ID,
                    ROSEA_COMMIT,
                    escape_json(&err_msg)
                );
            } else {
                eprintln!("rosea: {err_msg}");
            }
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!(
        "\
Rosea - osu! parser & simulator ({profile} @ {commit})

USAGE:
  rosea                                           Print version/profile and commit hash
  rosea <file.osu> [--replay <file.osr>] [--json] Parse beatmap and optional replay
  rosea parse <file.osu> [--replay <file.osr>] [--json]
  rosea --map <file.osu> [--replay <file.osr>] [--json]
  rosea --version                                 Show version and commit
  rosea --help                                    Show this help
",
        profile = rosea_core::PROFILE_ID,
        commit = ROSEA_COMMIT,
    );
}

type CliError = (&'static str, String, Option<String>);

fn read_limited(
    path: &Path,
    limit: u64,
    resource: &'static str,
) -> Result<Vec<u8>, (&'static str, String)> {
    let file = File::open(path).map_err(|e| {
        (
            "IoError",
            format!("Не удалось прочитать '{}': {e}", path.display()),
        )
    })?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| {
            (
                "IoError",
                format!("Не удалось прочитать '{}': {e}", path.display()),
            )
        })?;
    if bytes.len() as u64 > limit {
        let err = RoseaError::ResourceLimit {
            resource,
            limit,
            observed: bytes.len() as u64,
        };
        return Err((error_code(&err), err.to_string()));
    }
    Ok(bytes)
}

fn run_parse(path: &Path, replay_path: Option<&Path>, json: bool) -> Result<String, CliError> {
    let map_bytes = read_limited(path, MAX_OSU_FILE_BYTES, "osu_file_bytes")
        .map_err(|(code, msg)| (code, msg, None))?;

    let parsed_map = osu(&map_bytes).map_err(|e| {
        let replay_part = if json {
            replay_path.map(|rp| build_replay_json(&map_bytes, rp))
        } else {
            None
        };
        (error_code(&e), e.to_string(), replay_part)
    })?;

    if !json {
        let beatmap = &parsed_map.value;
        let md5 = md5_hex(beatmap_md5(&map_bytes));
        let mut summary = format!(
            "Beatmap v14 (mode 0, md5={md5}): {} timing points, {} hit objects, {} breaks",
            beatmap.timing_points().len(),
            beatmap.objects().len(),
            beatmap.breaks().len()
        );
        if let Some(rp) = replay_path {
            let replay_summary =
                run_replay_text(&map_bytes, rp).map_err(|(code, msg)| (code, msg, None))?;
            let _ = write!(summary, "\n{replay_summary}");
        }
        return Ok(summary);
    }

    let beatmap = &parsed_map.value;
    let md5 = md5_hex(beatmap_md5(&map_bytes));
    let replay_json = match replay_path {
        Some(rp) => build_replay_json(&map_bytes, rp),
        None => "null".to_string(),
    };

    Ok(format_beatmap_json(
        beatmap,
        &md5,
        &parsed_map.warnings,
        &replay_json,
    ))
}

fn run_replay_text(map_bytes: &[u8], replay_path: &Path) -> Result<String, (&'static str, String)> {
    let replay_bytes = read_limited(replay_path, MAX_OSR_FILE_BYTES, "osr_file_bytes")?;
    let parsed_replay = osr(&replay_bytes).map_err(|e| (error_code(&e), e.to_string()))?;
    check_beatmap_hash(map_bytes, &parsed_replay.value.meta)
        .map_err(|e| (error_code(&e), e.to_string()))?;
    let player = parsed_replay.value.meta.player;
    let expected = parsed_replay.value.expected;
    let normalized =
        normalize(parsed_replay.value.input).map_err(|e| (error_code(&e), e.to_string()))?;

    Ok(format!(
        "Replay by '{player}': score={}, max_combo={}, {}/{}/{}/{} ({} normalized frames)",
        expected.score,
        expected.max_combo,
        expected.count_300,
        expected.count_100,
        expected.count_50,
        expected.count_miss,
        normalized.value.record_count()
    ))
}

fn format_beatmap_json(
    beatmap: &Beatmap,
    md5: &str,
    warnings: &[Warning],
    replay_json: &str,
) -> String {
    let difficulty = beatmap.difficulty();
    let diff_json = format!(
        r#"{{"cs":{},"od":{},"hp":{},"ar":{},"slider_multiplier":{},"slider_tick_rate":{}}}"#,
        canon_f64(difficulty.cs),
        canon_f64(difficulty.od),
        canon_f64(difficulty.hp),
        canon_f64(difficulty.ar),
        difficulty
            .slider_multiplier
            .map(|v| canon_f64(v).to_string())
            .unwrap_or_else(|| "null".into()),
        difficulty
            .slider_tick_rate
            .map(|v| canon_f64(v).to_string())
            .unwrap_or_else(|| "null".into()),
    );

    let metadata_json = format_metadata_json(beatmap.metadata());
    let colours_json = format_colours_json(beatmap.colours());
    let breaks_json = format_breaks_json(beatmap.breaks());
    let tp_json = format_timing_points_json(beatmap);
    let ho_json = format_hit_objects_json(beatmap);

    let counts = beatmap.object_counts();
    let counts_json = format!(
        r#"{{"circles":{},"sliders_l":{},"sliders_b":{},"sliders_p":{},"spinners":{}}}"#,
        counts.circles, counts.sliders_l, counts.sliders_b, counts.sliders_p, counts.spinners
    );

    let warnings_json = format_warnings_json(warnings, 0);

    format!(
        r#"{{"status":"ok","profile":"{}","commit":"{}","beatmap_md5":"{}","version":14,"mode":0,"stack_leniency":{},"metadata":{},"colours":{},"breaks":{},"difficulty":{},"timing_points":{},"hit_objects":{},"object_counts":{},"warnings":{},"replay":{}}}"#,
        rosea_core::PROFILE_ID,
        ROSEA_COMMIT,
        md5,
        canon_f32(beatmap.stack_leniency()),
        metadata_json,
        colours_json,
        breaks_json,
        diff_json,
        tp_json,
        ho_json,
        counts_json,
        warnings_json,
        replay_json
    )
}

fn format_metadata_json(meta: &Metadata) -> String {
    format!(
        r#"{{"title":"{}","title_unicode":{},"artist":"{}","artist_unicode":{},"creator":"{}","version":"{}","source":"{}","tags":"{}","beatmap_id":{},"beatmap_set_id":{}}}"#,
        escape_json(&meta.title),
        opt_str_json(meta.title_unicode.as_deref()),
        escape_json(&meta.artist),
        opt_str_json(meta.artist_unicode.as_deref()),
        escape_json(&meta.creator),
        escape_json(&meta.version),
        escape_json(&meta.source),
        escape_json(&meta.tags),
        opt_i32_json(meta.beatmap_id),
        opt_i32_json(meta.beatmap_set_id),
    )
}

fn format_colours_json(colours: &Colours) -> String {
    let mut combo_json = String::from("[");
    for (i, rgb) in colours.combo.iter().enumerate() {
        if i > 0 {
            combo_json.push(',');
        }
        let _ = write!(combo_json, "{}", rgb_json(*rgb));
    }
    combo_json.push(']');

    format!(
        r#"{{"combo":{},"slider_track_override":{},"slider_border":{}}}"#,
        combo_json,
        opt_rgb_json(colours.slider_track_override),
        opt_rgb_json(colours.slider_border),
    )
}

fn format_breaks_json(breaks: &[BreakPeriod]) -> String {
    let mut out = String::with_capacity(2 + breaks.len() * 44);
    out.push('[');
    for (i, br) in breaks.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            r#"{{"start_time":{},"end_time":{}}}"#,
            canon_f64(br.start_time),
            canon_f64(br.end_time)
        );
    }
    out.push(']');
    out
}

fn format_timing_points_json(beatmap: &Beatmap) -> String {
    let timing_points = beatmap.timing_points();
    let mut tp_json = String::with_capacity(2 + timing_points.len() * 110);
    tp_json.push('[');
    for (i, tp) in timing_points.iter().enumerate() {
        if i > 0 {
            tp_json.push(',');
        }
        let _ = write!(
            tp_json,
            r#"{{"line":{},"time":{},"beat_length":{},"meter":{},"sample_set":{},"sample_index":{},"volume":{},"uninherited":{},"effects":{}}}"#,
            tp.line,
            canon_f64(tp.time),
            canon_f64(tp.beat_length),
            tp.meter,
            tp.sample_set,
            tp.sample_index,
            tp.volume,
            tp.uninherited,
            tp.effects
        );
    }
    tp_json.push(']');
    tp_json
}

fn format_hit_objects_json(beatmap: &Beatmap) -> String {
    let hit_objects = beatmap.objects();
    let mut ho_json = String::with_capacity(2 + hit_objects.len() * 120);
    ho_json.push('[');
    for (i, obj) in hit_objects.iter().enumerate() {
        if i > 0 {
            ho_json.push(',');
        }
        match &obj.kind {
            HitObjectKind::Circle => {
                let _ = write!(
                    ho_json,
                    r#"{{"id":{},"line":{},"x":{},"y":{},"time":{},"flags":{},"hit_sound":{},"kind":"circle"}}"#,
                    obj.id.0,
                    obj.line,
                    canon_f32(obj.position.x),
                    canon_f32(obj.position.y),
                    canon_f64(obj.start_time),
                    obj.flags,
                    obj.hit_sound
                );
            }
            HitObjectKind::Slider {
                kind,
                controls,
                spans,
                declared_length,
            } => {
                let curve_str = match kind {
                    CurveKind::Linear => "linear",
                    CurveKind::Bezier => "bezier",
                    CurveKind::PerfectCurve => "perfect",
                };
                let mut ctrl_json = String::with_capacity(2 + controls.len() * 20);
                ctrl_json.push('[');
                for (ci, p) in controls.iter().enumerate() {
                    if ci > 0 {
                        ctrl_json.push(',');
                    }
                    let _ = write!(
                        ctrl_json,
                        r#"{{"x":{},"y":{}}}"#,
                        canon_f32(p.x),
                        canon_f32(p.y)
                    );
                }
                ctrl_json.push(']');
                let _ = write!(
                    ho_json,
                    r#"{{"id":{},"line":{},"x":{},"y":{},"time":{},"flags":{},"hit_sound":{},"kind":"slider","curve_kind":"{}","controls":{},"spans":{},"declared_length":{}}}"#,
                    obj.id.0,
                    obj.line,
                    canon_f32(obj.position.x),
                    canon_f32(obj.position.y),
                    canon_f64(obj.start_time),
                    obj.flags,
                    obj.hit_sound,
                    curve_str,
                    ctrl_json,
                    spans,
                    canon_f64(*declared_length)
                );
            }
            HitObjectKind::Spinner { end } => {
                let _ = write!(
                    ho_json,
                    r#"{{"id":{},"line":{},"x":{},"y":{},"time":{},"flags":{},"hit_sound":{},"kind":"spinner","spinner_end_time":{}}}"#,
                    obj.id.0,
                    obj.line,
                    canon_f32(obj.position.x),
                    canon_f32(obj.position.y),
                    canon_f64(obj.start_time),
                    obj.flags,
                    obj.hit_sound,
                    canon_f64(*end)
                );
            }
        }
    }
    ho_json.push(']');
    ho_json
}

fn build_replay_json(map_bytes: &[u8], replay_path: &Path) -> String {
    let replay_bytes = match read_limited(replay_path, MAX_OSR_FILE_BYTES, "osr_file_bytes") {
        Ok(b) => b,
        Err((code, msg)) => {
            return format!(
                r#"{{"status":"error","error_code":"{}","error":"{}","hash_status":"unchecked","hash_match":null,"warnings":[]}}"#,
                escape_json(code),
                escape_json(&msg)
            );
        }
    };
    build_replay_json_from_bytes(map_bytes, &replay_bytes)
}

fn build_replay_json_from_bytes(map_bytes: &[u8], replay_bytes: &[u8]) -> String {
    let parsed_replay = match osr(replay_bytes) {
        Ok(p) => p,
        Err(e) => {
            return format!(
                r#"{{"status":"error","error_code":"{}","error":"{}","hash_status":"unchecked","hash_match":null,"warnings":[]}}"#,
                escape_json(error_code(&e)),
                escape_json(&e.to_string())
            );
        }
    };

    let meta = parsed_replay.value.meta;
    let expected = parsed_replay.value.expected;
    let input = parsed_replay.value.input;
    let seed = input.seed();
    let raw_record_count = input.records().len();
    let trailer_size = meta.trailer.len();

    let hash_err = check_beatmap_hash(map_bytes, &meta).err();
    let hash_match = hash_err.is_none();
    let hash_status = if hash_match {
        "ok"
    } else {
        "BeatmapHashMismatch"
    };

    let mut ts_json = String::from("[");
    for (idx, b) in meta.timestamp.iter().enumerate() {
        if idx > 0 {
            ts_json.push(',');
        }
        let _ = write!(ts_json, "{b}");
    }
    ts_json.push(']');

    let header_json = format!(
        r#"{{"mode":{},"game_version":{},"beatmap_hash":"{}","player":"{}","replay_hash":"{}","mods":{},"perfect":{},"count_geki":{},"count_katu":{},"life_graph":"{}","timestamp_bytes":{},"seed":{},"trailer_size":{},"raw_record_count":{}}}"#,
        meta.mode,
        meta.game_version(),
        escape_json(meta.beatmap_hash()),
        escape_json(&meta.player),
        escape_json(&meta.replay_hash),
        meta.mods,
        meta.perfect,
        meta.count_geki,
        meta.count_katu,
        escape_json(&meta.life_graph),
        ts_json,
        opt_i32_json(seed),
        trailer_size,
        raw_record_count
    );

    let expected_json = format!(
        r#"{{"score":{},"max_combo":{},"count_300":{},"count_100":{},"count_50":{},"count_miss":{}}}"#,
        expected.score,
        expected.max_combo,
        expected.count_300,
        expected.count_100,
        expected.count_50,
        expected.count_miss
    );

    let mut combined_warnings = parsed_replay.warnings;
    let norm_res = normalize(input);

    match (&hash_err, norm_res) {
        (None, Ok(norm_parsed)) => {
            combined_warnings.extend(norm_parsed.warnings);
            let norm_json = format_normalization_summary(&combined_warnings);
            let warnings_json = format_warnings_json(&combined_warnings, trailer_size);
            let frames_json = format_timeline_frames_json(&norm_parsed.value);
            format!(
                r#"{{"status":"ok","error_code":null,"error":null,"hash_status":"{hash_status}","hash_match":true,"header":{header_json},"expected":{expected_json},"warnings":{warnings_json},"normalization":{norm_json},"record_count":{},"group_count":{},"frames":{frames_json}}}"#,
                norm_parsed.value.record_count(),
                norm_parsed.value.groups().len()
            )
        }
        (Some(h_err), Ok(norm_parsed)) => {
            combined_warnings.extend(norm_parsed.warnings);
            let norm_json = format_normalization_summary(&combined_warnings);
            let warnings_json = format_warnings_json(&combined_warnings, trailer_size);
            format!(
                r#"{{"status":"error","error_code":"{}","error":"{}","hash_status":"{hash_status}","hash_match":false,"header":{header_json},"expected":{expected_json},"warnings":{warnings_json},"normalization":{norm_json},"record_count":0,"group_count":0,"frames":[]}}"#,
                escape_json(error_code(h_err)),
                escape_json(&h_err.to_string()),
            )
        }
        (Some(h_err), Err(_)) => {
            let warnings_json = format_warnings_json(&combined_warnings, trailer_size);
            format!(
                r#"{{"status":"error","error_code":"{}","error":"{}","hash_status":"{hash_status}","hash_match":false,"header":{header_json},"expected":{expected_json},"warnings":{warnings_json},"normalization":null,"record_count":0,"group_count":0,"frames":[]}}"#,
                escape_json(error_code(h_err)),
                escape_json(&h_err.to_string()),
            )
        }
        (None, Err(n_err)) => {
            let warnings_json = format_warnings_json(&combined_warnings, trailer_size);
            format!(
                r#"{{"status":"error","error_code":"{}","error":"{}","hash_status":"{hash_status}","hash_match":true,"header":{header_json},"expected":{expected_json},"warnings":{warnings_json},"normalization":null,"record_count":0,"group_count":0,"frames":[]}}"#,
                escape_json(error_code(&n_err)),
                escape_json(&n_err.to_string()),
            )
        }
    }
}

fn format_timeline_frames_json(timeline: &ReplayTimeline) -> String {
    let records = timeline.records();
    let mut out = String::with_capacity(2 + records.len() * 64);
    out.push('[');
    for (i, rec) in records.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            r#"{{"index":{},"time_ms":{},"x":{},"y":{},"keys":{}}}"#,
            rec.index,
            rec.time_ms,
            canon_f32(rec.cursor.x),
            canon_f32(rec.cursor.y),
            rec.keys
        );
    }
    out.push(']');
    out
}

fn format_normalization_summary(warnings: &[Warning]) -> String {
    for w in warnings {
        if let Warning::ReplayInputNormalized {
            changed_times,
            initial_dummy_removed,
            backward_removed,
            changes,
        } = w
        {
            return format!(
                r#"{{"changed_times":{changed_times},"initial_dummy_removed":{initial_dummy_removed},"backward_removed":{backward_removed},"changes":{}}}"#,
                format_replay_changes_json(changes)
            );
        }
    }
    r#"{"changed_times":0,"initial_dummy_removed":0,"backward_removed":0,"changes":[]}"#.to_string()
}

fn format_warnings_json(warnings: &[Warning], trailer_size: usize) -> String {
    let mut out = String::from("[");
    for (i, w) in warnings.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match w {
            Warning::ReplayTrailerNotInterpreted => {
                let _ = write!(
                    out,
                    r#"{{"code":"ReplayTrailerNotInterpreted","trailer_size":{trailer_size}}}"#
                );
            }
            Warning::UnknownBeatmapFields { count } => {
                let _ = write!(out, r#"{{"code":"UnknownBeatmapFields","count":{count}}}"#);
            }
            Warning::ReplayInputNormalized {
                changed_times,
                initial_dummy_removed,
                backward_removed,
                changes,
            } => {
                let _ = write!(
                    out,
                    r#"{{"code":"ReplayInputNormalized","changed_times":{changed_times},"initial_dummy_removed":{initial_dummy_removed},"backward_removed":{backward_removed},"changes":{}}}"#,
                    format_replay_changes_json(changes)
                );
            }
        }
    }
    out.push(']');
    out
}

fn format_replay_changes_json(changes: &[ReplayChange]) -> String {
    let mut out = String::with_capacity(2 + changes.len() * 48);
    out.push('[');
    for (i, ch) in changes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match ch {
            ReplayChange::Time {
                index,
                old_ms,
                new_ms,
            } => {
                let _ = write!(
                    out,
                    r#"{{"kind":"Time","index":{index},"old_ms":{old_ms},"new_ms":{new_ms}}}"#
                );
            }
            ReplayChange::InitialDummy { index } => {
                let _ = write!(out, r#"{{"kind":"InitialDummy","index":{index}}}"#);
            }
            ReplayChange::BackwardFrame { index } => {
                let _ = write!(out, r#"{{"kind":"BackwardFrame","index":{index}}}"#);
            }
        }
    }
    out.push(']');
    out
}

fn error_code(err: &RoseaError) -> &'static str {
    match err {
        RoseaError::UnexpectedEof { .. } => "UnexpectedEof",
        RoseaError::InvalidLength { .. } => "InvalidLength",
        RoseaError::InvalidStringMarker { .. } => "InvalidStringMarker",
        RoseaError::InvalidReplayString { .. } => "InvalidReplayString",
        RoseaError::UnsupportedReplayMode { .. } => "UnsupportedReplayMode",
        RoseaError::UnsupportedMods { .. } => "UnsupportedMods",
        RoseaError::NoReplayData { .. } => "NoReplayData",
        RoseaError::InvalidLzma { .. } => "InvalidLzma",
        RoseaError::InvalidReplayRecord { .. } => "InvalidReplayRecord",
        RoseaError::UnsupportedReplayFeature { .. } => "UnsupportedReplayFeature",
        RoseaError::ReplayTimeOverflow { .. } => "ReplayTimeOverflow",
        RoseaError::BeatmapHashMismatch => "BeatmapHashMismatch",
        RoseaError::InvalidEncoding => "InvalidEncoding",
        RoseaError::InvalidHeader => "InvalidHeader",
        RoseaError::UnsupportedBeatmapVersion(_) => "UnsupportedBeatmapVersion",
        RoseaError::InvalidMode => "InvalidMode",
        RoseaError::UnsupportedMode(_) => "UnsupportedMode",
        RoseaError::InvalidStackLeniency { .. } => "InvalidStackLeniency",
        RoseaError::DuplicateField => "DuplicateField",
        RoseaError::MissingDifficultyField(_) => "MissingDifficultyField",
        RoseaError::InvalidDifficultyField { .. } => "InvalidDifficultyField",
        RoseaError::InvalidMetadataField { .. } => "InvalidMetadataField",
        RoseaError::InvalidColourField { .. } => "InvalidColourField",
        RoseaError::EmptyBeatmap => "EmptyBeatmap",
        RoseaError::InvalidHitObjectField { .. } => "InvalidHitObjectField",
        RoseaError::UnsupportedHitObject { .. } => "UnsupportedHitObject",
        RoseaError::UnsupportedSliderSyntax { .. } => "UnsupportedSliderSyntax",
        RoseaError::InvalidTimingPointField { .. } => "InvalidTimingPointField",
        RoseaError::InvalidBreakField { .. } => "InvalidBreakField",
        RoseaError::InvalidSliderLength { .. } => "InvalidSliderLength",
        RoseaError::NumericalError { .. } => "NumericalError",
        RoseaError::PreprocessError { .. } => "PreprocessError",
        RoseaError::ResourceLimit { .. } => "ResourceLimit",
    }
}

fn md5_hex(hash: [u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in hash {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn canon_f64(v: f64) -> f64 {
    if v == 0.0 { 0.0 } else { v }
}

fn canon_f32(v: f32) -> f32 {
    if v == 0.0 { 0.0 } else { v }
}

fn opt_str_json(val: Option<&str>) -> String {
    match val {
        Some(s) => format!(r#""{}""#, escape_json(s)),
        None => "null".to_string(),
    }
}

fn opt_i32_json(val: Option<i32>) -> String {
    match val {
        Some(v) => v.to_string(),
        None => "null".to_string(),
    }
}

fn rgb_json(rgb: Rgb) -> String {
    format!(r#"{{"r":{},"g":{},"b":{}}}"#, rgb.r, rgb.g, rgb.b)
}

fn opt_rgb_json(rgb: Option<Rgb>) -> String {
    match rgb {
        Some(c) => rgb_json(c),
        None => "null".to_string(),
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_beatmap_json_with_metadata_colours_breaks_and_warnings() {
        let map_bytes = b"\xef\xbb\xbfosu file format v14\n\
            [General]\nMode: 0\nStackLeniency: 0.5\nAudioFilename: song.mp3\n\
            [Metadata]\nTitle: Song\nTitleUnicode: \xd0\x9f\xd0\xb5\xd1\x81\xd0\xbd\xd1\x8f\n\
            Artist: Art\nCreator: Mapper\nVersion: Hard\nBeatmapID: 101\nBeatmapSetID: 202\n\
            [Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n\
            [Events]\n2,1000,2500\n\
            [Colours]\nCombo1: 10,20,30\nSliderTrackOverride: 40,50,60\nSliderBorder: 70,80,90\n\
            [HitObjects]\n256,192,500,1,0\n";
        let parsed = osu(map_bytes).unwrap();
        let md5 = md5_hex(beatmap_md5(map_bytes));
        let json = format_beatmap_json(&parsed.value, &md5, &parsed.warnings, "null");

        assert!(json.contains(&format!(r#""beatmap_md5":"{md5}""#)));
        assert!(json.contains(r#""stack_leniency":0.5"#));
        assert!(json.contains(r#""title":"Song""#));
        assert!(json.contains(r#""title_unicode":"Песня""#));
        assert!(json.contains(r#""beatmap_id":101,"beatmap_set_id":202"#));
        assert!(json.contains(r#""slider_track_override":{"r":40,"g":50,"b":60}"#));
        assert!(json.contains(r#""slider_border":{"r":70,"g":80,"b":90}"#));
        assert!(json.contains(r#""breaks":[{"start_time":1000,"end_time":2500}]"#));
        assert!(json.contains(r#""code":"UnknownBeatmapFields","count":1"#));
    }

    #[test]
    fn builds_replay_json_with_hash_check_and_normalization() {
        let map_bytes = b"osu file format v14\n[General]\nMode: 0\n\
            [Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n\
            [HitObjects]\n256,192,1000,1,0\n";
        let md5 = md5_hex(beatmap_md5(map_bytes));

        // Сжатый LZMA-поток с одной записью 10|256|192|5, и хвост [7, 8].
        let data = [
            93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 12, 3, 65, 4,
            153, 91, 83, 160, 122, 219, 176, 37, 27, 45, 1, 127, 255, 225, 190, 128, 0,
        ];
        let mut osr_bytes = vec![0_u8];
        osr_bytes.extend_from_slice(&20240101_i32.to_le_bytes());
        osr_bytes.extend_from_slice(&[0x0b, md5.len() as u8]);
        osr_bytes.extend_from_slice(md5.as_bytes());
        osr_bytes.extend_from_slice(&[0x0b, 4, b'T', b'e', b's', b't', 0]);
        for c in [10_u16, 2, 1, 3, 4, 0] {
            osr_bytes.extend_from_slice(&c.to_le_bytes());
        }
        osr_bytes.extend_from_slice(&12345_i32.to_le_bytes());
        osr_bytes.extend_from_slice(&42_u16.to_le_bytes());
        osr_bytes.push(1);
        osr_bytes.extend_from_slice(&0_u32.to_le_bytes());
        osr_bytes.push(0);
        osr_bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        osr_bytes.extend_from_slice(&(data.len() as i32).to_le_bytes());
        osr_bytes.extend_from_slice(&data);
        osr_bytes.extend_from_slice(&[7, 8]);

        let json_ok = build_replay_json_from_bytes(map_bytes, &osr_bytes);
        assert!(json_ok.contains(r#""status":"ok""#));
        assert!(json_ok.contains(r#""hash_status":"ok","hash_match":true"#));
        assert!(json_ok.contains(r#""player":"Test""#));
        assert!(json_ok.contains(r#""trailer_size":2"#));
        assert!(json_ok.contains(r#""code":"ReplayTrailerNotInterpreted","trailer_size":2"#));
        assert!(
            json_ok.contains(r#""frames":[{"index":0,"time_ms":10,"x":256,"y":192,"keys":5}]"#)
        );

        // При изменении карты должен сработать BeatmapHashMismatch, а frames остаться пустым.
        let other_map = b"osu file format v14\n[General]\nMode: 0\n\
            [Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n\
            [HitObjects]\n256,192,1001,1,0\n";
        let json_mismatch = build_replay_json_from_bytes(other_map, &osr_bytes);
        assert!(json_mismatch.contains(r#""status":"error","error_code":"BeatmapHashMismatch""#));
        assert!(
            json_mismatch.contains(r#""hash_status":"BeatmapHashMismatch","hash_match":false"#)
        );
        assert!(json_mismatch.contains(r#""record_count":0,"group_count":0,"frames":[]"#));
    }

    #[test]
    fn parses_cli_flags_and_reports_usage_errors() {
        let missing_map = parse_cli_args([
            OsString::from("rosea"),
            OsString::from("--replay"),
            OsString::from("r.osr"),
            OsString::from("--json"),
        ]);
        assert!(matches!(missing_map, Err((_, true))));

        let missing_val = parse_cli_args([
            OsString::from("rosea"),
            OsString::from("--json"),
            OsString::from("--map"),
        ]);
        assert!(matches!(missing_val, Err((_, true))));

        let bad_flag_before_json = parse_cli_args([
            OsString::from("rosea"),
            OsString::from("--unknown"),
            OsString::from("--json"),
        ]);
        assert!(matches!(bad_flag_before_json, Err((_, true))));

        let ok = parse_cli_args([
            OsString::from("rosea"),
            OsString::from("--map"),
            OsString::from("a b.osu"),
            OsString::from("--replay"),
            OsString::from("c d.osr"),
            OsString::from("--json"),
        ]);
        assert!(matches!(
            ok,
            Ok(CliAction::Parse {
                replay_path: Some(_),
                json: true,
                ..
            })
        ));
    }

    #[test]
    fn read_limited_enforces_exact_byte_cap() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("rosea_limit_test_{}.bin", std::process::id()));
        std::fs::write(&path, b"12345").unwrap();
        assert_eq!(read_limited(&path, 5, "test").unwrap(), b"12345");
        let err = read_limited(&path, 4, "test").unwrap_err();
        assert_eq!(err.0, "ResourceLimit");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn escapes_control_characters_in_json() {
        assert_eq!(escape_json("a\"b\\c\n\r\t\x08"), r#"a\"b\\c\n\r\t\u0008"#);
    }

    #[test]
    fn formats_normalization_and_replay_warnings_json() {
        let warnings = vec![
            Warning::ReplayTrailerNotInterpreted,
            Warning::ReplayInputNormalized {
                changed_times: 2,
                initial_dummy_removed: 1,
                backward_removed: 1,
                changes: vec![
                    ReplayChange::Time {
                        index: 1,
                        old_ms: -10,
                        new_ms: 5,
                    },
                    ReplayChange::InitialDummy { index: 0 },
                    ReplayChange::BackwardFrame { index: 3 },
                ],
            },
        ];
        let summary = format_normalization_summary(&warnings);
        assert!(
            summary.contains(r#""changed_times":2,"initial_dummy_removed":1,"backward_removed":1"#)
        );
        assert!(summary.contains(r#"{"kind":"Time","index":1,"old_ms":-10,"new_ms":5}"#));
        assert!(summary.contains(r#"{"kind":"InitialDummy","index":0}"#));
        assert!(summary.contains(r#"{"kind":"BackwardFrame","index":3}"#));

        let warnings_json = format_warnings_json(&warnings, 8);
        assert!(
            warnings_json.contains(r#"{"code":"ReplayTrailerNotInterpreted","trailer_size":8}"#)
        );
        assert!(warnings_json.contains(r#""code":"ReplayInputNormalized""#));
    }
}
