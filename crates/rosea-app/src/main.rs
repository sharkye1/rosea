//! Точка входа приложения Rosea. Поддерживает вывод профиля и разбор карт в JSON.

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use rosea_core::model::{CurveKind, HitObjectKind};
use rosea_core::parse::{osu_difficulty, osu_header, osu_hit_objects, osu_mode, osu_timing_points};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        println!("Rosea ({})", rosea_core::PROFILE_ID);
        return ExitCode::SUCCESS;
    }

    let mut path: Option<&str> = None;
    let mut json_output = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" | "help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "--json" => {
                json_output = true;
            }
            "parse" | "dump" => {
                // optional subcommand
            }
            _ => {
                if !arg.starts_with('-') && path.is_none() {
                    path = Some(arg);
                } else if arg == "--map" {
                    i += 1;
                    if i < args.len() {
                        path = Some(&args[i]);
                    }
                }
            }
        }
        i += 1;
    }

    let Some(file_path) = path else {
        eprintln!("rosea: no beatmap file specified. Run with --help for usage.");
        return ExitCode::from(2);
    };

    match parse_beatmap_file(file_path, json_output) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(err_msg) => {
            if json_output {
                println!(
                    r#"{{"status":"error","profile":"{}","error":"{}"}}"#,
                    rosea_core::PROFILE_ID,
                    escape_json(&err_msg)
                );
            } else {
                eprintln!("rosea error: {err_msg}");
            }
            ExitCode::from(1)
        }
    }
}

fn print_help() {
    println!(
        "\
Rosea - osu! parser & simulator ({profile})

USAGE:
  rosea                     Print version/profile
  rosea <file.osu> [--json] Parse and output beatmap structure
  rosea parse <file.osu> [--json]
  rosea --help              Show this help
",
        profile = rosea_core::PROFILE_ID
    );
}

fn parse_beatmap_file(path_str: &str, json: bool) -> Result<String, String> {
    let path = Path::new(path_str);
    let bytes = fs::read(path).map_err(|e| format!("Failed to read '{path_str}': {e}"))?;

    let version = osu_header(&bytes).map_err(|e| e.to_string())?;
    let mode = osu_mode(&bytes).map_err(|e| e.to_string())?;
    let difficulty = osu_difficulty(&bytes).map_err(|e| e.to_string())?;
    let timing_points = osu_timing_points(&bytes).map_err(|e| e.to_string())?;
    let hit_objects = osu_hit_objects(&bytes).map_err(|e| e.to_string())?;

    if json {
        let diff_json = format!(
            r#"{{"cs":{},"od":{},"hp":{},"ar":{},"slider_multiplier":{},"slider_tick_rate":{}}}"#,
            difficulty.cs,
            difficulty.od,
            difficulty.hp,
            difficulty.ar,
            difficulty
                .slider_multiplier
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into()),
            difficulty
                .slider_tick_rate
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into()),
        );

        let mut tp_json = String::from("[");
        for (i, tp) in timing_points.iter().enumerate() {
            if i > 0 {
                tp_json.push(',');
            }
            tp_json.push_str(&format!(
                r#"{{"line":{},"time":{},"beat_length":{},"meter":{},"sample_set":{},"sample_index":{},"volume":{},"uninherited":{},"effects":{}}}"#,
                tp.line,
                tp.time,
                tp.beat_length,
                tp.meter,
                tp.sample_set,
                tp.sample_index,
                tp.volume,
                tp.uninherited,
                tp.effects
            ));
        }
        tp_json.push(']');

        let mut ho_json = String::from("[");
        for (i, obj) in hit_objects.iter().enumerate() {
            if i > 0 {
                ho_json.push(',');
            }
            match &obj.kind {
                HitObjectKind::Circle => {
                    ho_json.push_str(&format!(
                        r#"{{"id":{},"line":{},"x":{},"y":{},"time":{},"flags":{},"hit_sound":{},"kind":"circle"}}"#,
                        obj.id.0,
                        obj.line,
                        obj.position.x,
                        obj.position.y,
                        obj.start_time,
                        obj.flags,
                        obj.hit_sound
                    ));
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
                    let mut ctrl_json = String::from("[");
                    for (ci, p) in controls.iter().enumerate() {
                        if ci > 0 {
                            ctrl_json.push(',');
                        }
                        ctrl_json.push_str(&format!(r#"{{"x":{},"y":{}}}"#, p.x, p.y));
                    }
                    ctrl_json.push(']');
                    ho_json.push_str(&format!(
                        r#"{{"id":{},"line":{},"x":{},"y":{},"time":{},"flags":{},"hit_sound":{},"kind":"slider","curve_kind":"{}","controls":{},"spans":{},"declared_length":{}}}"#,
                        obj.id.0,
                        obj.line,
                        obj.position.x,
                        obj.position.y,
                        obj.start_time,
                        obj.flags,
                        obj.hit_sound,
                        curve_str,
                        ctrl_json,
                        spans,
                        declared_length
                    ));
                }
                HitObjectKind::Spinner { end } => {
                    ho_json.push_str(&format!(
                        r#"{{"id":{},"line":{},"x":{},"y":{},"time":{},"flags":{},"hit_sound":{},"kind":"spinner","spinner_end_time":{}}}"#,
                        obj.id.0,
                        obj.line,
                        obj.position.x,
                        obj.position.y,
                        obj.start_time,
                        obj.flags,
                        obj.hit_sound,
                        end
                    ));
                }
            }
        }
        ho_json.push(']');

        Ok(format!(
            r#"{{"status":"ok","profile":"{}","version":{},"mode":{},"difficulty":{},"timing_points":{},"hit_objects":{}}}"#,
            rosea_core::PROFILE_ID,
            version,
            mode,
            diff_json,
            tp_json,
            ho_json
        ))
    } else {
        Ok(format!(
            "Beatmap v{version} (mode {mode}): {} timing points, {} hit objects",
            timing_points.len(),
            hit_objects.len()
        ))
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
            _ => out.push(c),
        }
    }
    out
}
