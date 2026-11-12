"""
Bridge to invoke and verify the rosea Rust parser against osu! beatmaps and replays.
"""

import hashlib
import json
import os
import subprocess
import sys
from typing import Any, Dict, List, Optional, Tuple

# Ensure repository root and scripts directory are in sys.path
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
for p in (REPO_ROOT, SCRIPT_DIR):
    if p not in sys.path:
        sys.path.insert(0, p)

try:
    from osu_parser import (
        Beatmap,
        HitObject,
        Replay,
        ReplayFrame,
        TimingPoint,
        apply_stacking,
        generate_slider_path,
        parse_osr_replay,
        parse_osu_beatmap,
    )
except ImportError:
    from scripts.osu_parser import (
        Beatmap,
        HitObject,
        Replay,
        ReplayFrame,
        TimingPoint,
        apply_stacking,
        generate_slider_path,
        parse_osr_replay,
        parse_osu_beatmap,
    )

_ROSEA_EXE_CHECKED = False


def get_rosea_executable() -> str:
    """Find or build the rosea binary, ensuring it matches current Rust sources."""
    global _ROSEA_EXE_CHECKED
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    target_exe = os.path.join(root_dir, "target", "debug", "rosea.exe")
    if not os.path.isfile(target_exe) and os.name != "nt":
        target_exe = os.path.join(root_dir, "target", "debug", "rosea")

    if not _ROSEA_EXE_CHECKED:
        if not os.path.isfile(target_exe):
            print("Building rosea-app...", file=sys.stderr)
            subprocess.run(["cargo", "build", "-p", "rosea-app"], cwd=root_dir, check=True)
        else:
            # Rebuild once per process so branch switches/edits never reuse a stale binary.
            try:
                subprocess.run(
                    ["cargo", "build", "-p", "rosea-app", "--quiet"],
                    cwd=root_dir,
                    check=True,
                )
            except OSError:
                pass
            except subprocess.CalledProcessError as e:
                print(
                    f"Warning: failed to rebuild rosea-app ({e}); using existing binary",
                    file=sys.stderr,
                )
        _ROSEA_EXE_CHECKED = True

    return target_exe


def get_rust_commit_hash() -> str:
    """Retrieve the Git commit hash of the compiled rosea Rust parser."""
    try:
        exe = get_rosea_executable()
        out = subprocess.check_output(
            [exe, "--version"], text=True, stderr=subprocess.DEVNULL
        ).strip()
        if "@" in out:
            c = out.split("@")[-1].strip()
            if c:
                return c
    except Exception:
        pass

    try:
        root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        out = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=root_dir,
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        if out:
            return out
    except Exception:
        pass

    return "unknown"


def run_rust_parser(
    file_path: str, replay_path: Optional[str] = None
) -> Dict[str, Any]:
    """Run rosea on file_path (and optional replay_path) with --json."""
    exe = get_rosea_executable()
    cmd = [exe, "--map", file_path, "--json"]
    if replay_path:
        cmd.extend(["--replay", replay_path])
    proc = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    stdout = proc.stdout.strip()
    if not stdout:
        return {
            "status": "error",
            "error": proc.stderr.strip() or f"Process exited with code {proc.returncode}",
        }
    try:
        return json.loads(stdout)
    except json.JSONDecodeError as e:
        return {
            "status": "error",
            "error": f"Failed to parse JSON output: {e}\nRaw output: {stdout[:200]}",
        }


def read_osu_audio_fields(file_path: str) -> Dict[str, Any]:
    """Read only audio-related [General] keys omitted by rosea-core spec."""
    audio_info: Dict[str, Any] = {
        "audio_filename": "",
        "audio_lead_in": 0,
        "preview_time": 0,
    }
    if not file_path or not os.path.isfile(file_path):
        return audio_info

    try:
        with open(file_path, "r", encoding="utf-8", errors="replace") as f:
            in_general = False
            for raw in f:
                line = raw.strip()
                if not line or line.startswith("//"):
                    continue
                if line.startswith("[") and line.endswith("]"):
                    in_general = line == "[General]"
                    continue
                if not in_general or ":" not in line:
                    continue
                k, v = line.split(":", 1)
                k, v = k.strip(), v.strip()
                if k == "AudioFilename":
                    audio_info["audio_filename"] = v
                elif k == "AudioLeadIn":
                    try:
                        audio_info["audio_lead_in"] = int(float(v))
                    except ValueError:
                        pass
                elif k == "PreviewTime":
                    try:
                        audio_info["preview_time"] = int(float(v))
                    except ValueError:
                        pass
    except Exception:
        pass
    return audio_info


def _rgb_tuple(obj: Optional[Dict[str, Any]]) -> Optional[Tuple[int, int, int]]:
    if obj is None:
        return None
    return (int(obj["r"]), int(obj["g"]), int(obj["b"]))


def _read_osu_lines_map(file_path: str) -> Dict[int, str]:
    """Read 1-based line number -> trimmed line text from a .osu file."""
    lines_map: Dict[int, str] = {}
    if not file_path or not os.path.isfile(file_path):
        return lines_map
    try:
        with open(file_path, "r", encoding="utf-8", errors="replace") as f:
            for idx, raw in enumerate(f, 1):
                lines_map[idx] = raw.strip()
    except Exception:
        pass
    return lines_map


def scan_unknown_beatmap_fields(file_path: str) -> List[Dict[str, Any]]:
    """Scan unknown sections and skipped .osu fields matching rosea-core::parse::osu."""
    if not file_path or not os.path.isfile(file_path):
        return []
    known_sections = {
        "[General]",
        "[Metadata]",
        "[Difficulty]",
        "[TimingPoints]",
        "[Colours]",
        "[HitObjects]",
        "[Events]",
    }
    known_keys = {
        "[General]": {"Mode", "StackLeniency"},
        "[Metadata]": {
            "Title",
            "TitleUnicode",
            "Artist",
            "ArtistUnicode",
            "Creator",
            "Version",
            "Source",
            "Tags",
            "BeatmapID",
            "BeatmapSetID",
        },
        "[Difficulty]": {
            "CircleSize",
            "OverallDifficulty",
            "HPDrainRate",
            "ApproachRate",
            "SliderMultiplier",
            "SliderTickRate",
        },
        "[Colours]": {
            "Combo1",
            "Combo2",
            "Combo3",
            "Combo4",
            "Combo5",
            "Combo6",
            "Combo7",
            "Combo8",
            "SliderTrackOverride",
            "SliderBorder",
        },
    }
    entries: List[Dict[str, Any]] = []
    section = ""
    try:
        with open(file_path, "r", encoding="utf-8", errors="replace") as f:
            for idx, raw in enumerate(f, 1):
                line = raw.strip()
                if not line or line.startswith("//"):
                    continue
                if line.startswith("["):
                    section = line
                    if section not in known_sections:
                        entries.append(
                            {
                                "line": idx,
                                "section": section,
                                "kind": "section",
                                "key": section,
                                "text": line[:160],
                            }
                        )
                    continue
                if section in known_keys:
                    key = line.split(":", 1)[0].strip() if ":" in line else line
                    if key not in known_keys[section]:
                        entries.append(
                            {
                                "line": idx,
                                "section": section,
                                "kind": "field",
                                "key": key,
                                "text": line[:160],
                            }
                        )
    except Exception:
        pass
    return entries


def _try_parse_float(token: str) -> Optional[float]:
    try:
        import math

        v = float(token.strip().strip('"'))
        return v if math.isfinite(v) else None
    except ValueError:
        return None


def build_map_issues(
    file_path: str,
    py_bm: Optional[Beatmap],
    rust_out: Optional[Dict[str, Any]],
    mismatches: List[str],
) -> List[Dict[str, Any]]:
    """Build jumpable list of map errors, unsupported objects, and mismatches."""
    issues: List[Dict[str, Any]] = []
    seen_keys = set()
    primary_locs = set()
    lines_map = _read_osu_lines_map(file_path)

    objs_by_id: Dict[int, HitObject] = {}
    objs_by_line: Dict[int, HitObject] = {}
    if py_bm:
        for o in py_bm.hit_objects:
            objs_by_id[o.id] = o
            objs_by_line[o.line] = o

    def add_issue(
        tag: str,
        summary: str,
        line: Optional[int] = None,
        object_id: Optional[int] = None,
        time_ms: Optional[float] = None,
        x: Optional[float] = None,
        y: Optional[float] = None,
        raw_line: str = "",
        primary: bool = False,
    ):
        key = (tag, line, object_id, summary)
        if key in seen_keys:
            return
        seen_keys.add(key)
        if not raw_line and line is not None:
            raw_line = lines_map.get(line, "")
        issues.append(
            {
                "tag": tag,
                "summary": summary,
                "line": line,
                "object_id": object_id,
                "time_ms": time_ms,
                "x": x,
                "y": y,
                "raw_line": raw_line[:160],
                "primary": primary,
            }
        )

    # 1. Exact Rust parser stop / error_location
    if rust_out and rust_out.get("status") != "ok":
        err_code = str(rust_out.get("error_code") or "RustError")
        err_msg = str(rust_out.get("error") or err_code)
        loc = rust_out.get("error_location") or {}
        loc_kind = loc.get("kind")
        line = loc.get("line")
        obj_id = loc.get("object_id")
        raw_line = lines_map.get(line, "") if isinstance(line, int) else ""

        t_ms: Optional[float] = None
        px: Optional[float] = None
        py: Optional[float] = None
        if loc_kind == "hit_object":
            obj = (
                objs_by_id.get(int(obj_id))
                if obj_id is not None
                else None
            ) or (objs_by_line.get(int(line)) if isinstance(line, int) else None)
            if obj is not None:
                t_ms = float(obj.time)
                px = float(obj.stacked_x)
                py = float(obj.stacked_y)
            elif raw_line:
                parts = raw_line.split(",")
                if len(parts) >= 3:
                    px = _try_parse_float(parts[0])
                    py = _try_parse_float(parts[1])
                    t_ms = _try_parse_float(parts[2])
        elif loc_kind == "timing_point" and raw_line:
            parts = raw_line.split(",")
            if parts:
                t_ms = _try_parse_float(parts[0])
        elif loc_kind == "break" and raw_line:
            parts = raw_line.split(",")
            if len(parts) >= 2:
                t_ms = _try_parse_float(parts[1])

        add_issue(
            tag=f"RUST: {err_code}",
            summary=err_msg,
            line=line,
            object_id=obj_id,
            time_ms=t_ms,
            x=px,
            y=py,
            raw_line=raw_line,
            primary=True,
        )
        if line is not None:
            primary_locs.add(("line", line))
        if obj_id is not None:
            primary_locs.add(("obj", obj_id))

    # 2. All Catmull sliders and other unsupported objects across the entire map
    if py_bm:
        for obj in py_bm.hit_objects:
            if obj.kind == "slider" and obj.curve_type == "C":
                if ("line", obj.line) in primary_locs or ("obj", obj.id) in primary_locs:
                    continue
                add_issue(
                    tag="Catmull (C)",
                    summary=f"Слайдер Catmull на строке {obj.line} (#{obj.id}) не поддерживается ядром Rust",
                    line=obj.line,
                    object_id=obj.id,
                    time_ms=float(obj.time),
                    x=float(obj.stacked_x),
                    y=float(obj.stacked_y),
                    raw_line=lines_map.get(obj.line, ""),
                )

        for entry in getattr(py_bm, "unsupported_objects", []):
            line_num, raw_line, reason = entry
            if ("line", line_num) in primary_locs:
                continue
            parts = raw_line.split(",")
            px = _try_parse_float(parts[0]) if len(parts) >= 1 else None
            py = _try_parse_float(parts[1]) if len(parts) >= 2 else None
            t_ms = _try_parse_float(parts[2]) if len(parts) >= 3 else None
            add_issue(
                tag="Unsupported",
                summary=f"Строка {line_num}: {reason}",
                line=line_num,
                object_id=None,
                time_ms=t_ms,
                x=px,
                y=py,
                raw_line=raw_line,
            )

    # 3. Discrepancies between Python and Rust parsers
    for msg in mismatches:
        line_num = None
        obj_id = None
        t_ms = None
        px = None
        py = None
        if py_bm:
            if msg.startswith("Obj["):
                try:
                    idx = int(msg.split("[", 1)[1].split("]", 1)[0])
                    if 0 <= idx < len(py_bm.hit_objects):
                        obj = py_bm.hit_objects[idx]
                        line_num = obj.line
                        obj_id = obj.id
                        t_ms = float(obj.time)
                        px = float(obj.stacked_x)
                        py = float(obj.stacked_y)
                except (ValueError, IndexError):
                    pass
            elif msg.startswith("TP["):
                try:
                    idx = int(msg.split("[", 1)[1].split("]", 1)[0])
                    if 0 <= idx < len(py_bm.timing_points):
                        tp = py_bm.timing_points[idx]
                        line_num = tp.line
                        t_ms = float(tp.time)
                except (ValueError, IndexError):
                    pass
            elif msg.startswith("Break["):
                try:
                    idx = int(msg.split("[", 1)[1].split("]", 1)[0])
                    if 0 <= idx < len(py_bm.breaks):
                        t_ms = float(py_bm.breaks[idx][0])
                except (ValueError, IndexError):
                    pass
        add_issue(
            tag="Mismatch",
            summary=msg,
            line=line_num,
            object_id=obj_id,
            time_ms=t_ms,
            x=px,
            y=py,
        )

    return issues


def build_replay_issues(
    py_rep: Optional[Replay],
    rust_replay: Optional[Replay],
    r_rep: Optional[Dict[str, Any]],
    mismatches: List[str],
    error_code: Optional[str],
    error_location: Optional[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    """Build jumpable list of replay normalization events and errors."""
    issues: List[Dict[str, Any]] = []
    raw_by_idx: Dict[int, ReplayFrame] = {}
    if py_rep:
        for seq_idx, fr in enumerate(py_rep.frames):
            raw_by_idx[fr.index if fr.index is not None else seq_idx] = fr
    rust_by_idx: Dict[int, ReplayFrame] = {}
    if rust_replay:
        for fr in rust_replay.frames:
            if fr.index is not None:
                rust_by_idx[fr.index] = fr

    first_kept_t = (
        float(rust_replay.frames[0].time_ms)
        if (rust_replay and rust_replay.frames)
        else (float(py_rep.frames[0].time_ms) if (py_rep and py_rep.frames) else 0.0)
    )

    def _nearest_kept_time(raw_index: int) -> float:
        if rust_replay and rust_replay.frames:
            prev_t = first_kept_t
            for fr in rust_replay.frames:
                if fr.index is not None and fr.index <= raw_index:
                    prev_t = float(fr.time_ms)
                elif fr.index is not None and fr.index > raw_index:
                    return prev_t
            return prev_t
        return first_kept_t

    # 1. Replay error / rejection point
    if r_rep and r_rep.get("status") != "ok":
        err_msg = str(r_rep.get("error") or error_code or "ReplayError")
        loc = error_location or {}
        idx = loc.get("index")
        t_ms: Optional[float] = None
        px: Optional[float] = None
        py: Optional[float] = None
        if isinstance(idx, int) and idx in raw_by_idx:
            fr = raw_by_idx[idx]
            t_ms = float(fr.time_ms)
            px = float(fr.x)
            py = float(fr.y)
        elif py_rep and py_rep.frames:
            fr = py_rep.frames[-1] if isinstance(idx, int) else py_rep.frames[0]
            t_ms = float(fr.time_ms)
            px = float(fr.x)
            py = float(fr.y)

        issues.append(
            {
                "tag": f"RUST: {error_code or 'Error'}",
                "summary": err_msg,
                "index": idx,
                "time_ms": t_ms,
                "x": px,
                "y": py,
                "detail": f"offset={loc.get('offset')}" if "offset" in loc else "",
                "primary": True,
            }
        )

    # 2. All normalization events (Time / InitialDummy / BackwardFrame)
    norm = (r_rep or {}).get("normalization") or {}
    for ch in norm.get("changes", []):
        kind = str(ch.get("kind", ""))
        idx = ch.get("index")
        r_fr = rust_by_idx.get(idx) if isinstance(idx, int) else None
        p_fr = raw_by_idx.get(idx) if isinstance(idx, int) else None
        if kind == "Time":
            old_ms = ch.get("old_ms", 0)
            new_ms = ch.get("new_ms", 0)
            t_ms = float(new_ms)
            px = float(r_fr.x) if r_fr else (float(p_fr.x) if p_fr else None)
            py = float(r_fr.y) if r_fr else (float(p_fr.y) if p_fr else None)
            issues.append(
                {
                    "tag": "Time",
                    "summary": f"Кадр #{idx}: время {old_ms} -> {new_ms} мс",
                    "index": idx,
                    "time_ms": t_ms,
                    "x": px,
                    "y": py,
                    "detail": f"coords=({px:.1f}, {py:.1f})" if px is not None and py is not None else "",
                    "primary": False,
                }
            )
        elif kind == "InitialDummy":
            px = float(p_fr.x) if p_fr else 256.0
            py = float(p_fr.y) if p_fr else -500.0
            t_ms = first_kept_t
            issues.append(
                {
                    "tag": "InitialDummy",
                    "summary": f"Кадр #{idx}: удалён стартовый dummy ({px:.0f}, {py:.0f})",
                    "index": idx,
                    "time_ms": t_ms,
                    "x": px,
                    "y": py,
                    "detail": f"переход к началу ({t_ms:.0f} мс)",
                    "primary": False,
                }
            )
        elif kind == "BackwardFrame":
            px = float(p_fr.x) if p_fr else None
            py = float(p_fr.y) if p_fr else None
            raw_t = p_fr.time_ms if p_fr else None
            t_ms = _nearest_kept_time(int(idx)) if isinstance(idx, int) else first_kept_t
            t_note = f"сырое время {raw_t:.0f} мс, " if raw_t is not None else ""
            issues.append(
                {
                    "tag": "BackwardFrame",
                    "summary": f"Кадр #{idx}: удалён обратный ход ({t_note}кадр пропущен)",
                    "index": idx,
                    "time_ms": t_ms,
                    "x": px,
                    "y": py,
                    "detail": f"рядом с {t_ms:.0f} мс",
                    "primary": False,
                }
            )

    # 3. Mismatches
    for msg in mismatches:
        issues.append(
            {
                "tag": "Mismatch",
                "summary": msg,
                "index": None,
                "time_ms": first_kept_t if py_rep else None,
                "x": None,
                "y": None,
                "detail": "",
                "primary": False,
            }
        )

    return issues


class ReplayVerificationResult:
    def __init__(self, beatmap_path: str, replay_path: Optional[str]):
        self.beatmap_path = beatmap_path
        self.replay_path = replay_path
        self.status = "NONE"  # "NONE", "PASS", "SKIPPED_UNSUPPORTED", "HASH_MISMATCH", "FAIL"
        self.reason = "No replay"
        self.error_code: Optional[str] = None
        self.error_location: Optional[Dict[str, Any]] = None
        self.hash_status = "none"
        self.hash_match: Optional[bool] = None
        self.mismatches: List[str] = []
        self.warnings: List[Dict[str, Any]] = []
        self.normalization: Optional[Dict[str, Any]] = None
        self.trailer_offset: int = 0
        self.trailer_hex: str = ""
        self.trailer_ascii: str = ""
        self.trailer_i64_le: Optional[int] = None
        self.replay_issues: List[Dict[str, Any]] = []
        self.rust_replay_data: Optional[Dict[str, Any]] = None
        self.py_replay: Optional[Replay] = None
        self.rust_replay: Optional[Replay] = None


class VerificationResult:
    def __init__(self, file_path: str):
        self.file_path = file_path
        self.status = "UNKNOWN"  # "PASS", "SKIPPED_UNSUPPORTED", "FAIL"
        self.reason = ""
        self.error_code: Optional[str] = None
        self.error_location: Optional[Dict[str, Any]] = None
        self.mismatches: List[str] = []
        self.objects_checked = 0
        self.rust_supported = False
        self.py_beatmap: Optional[Beatmap] = None
        self.rust_data: Optional[Dict[str, Any]] = None
        self.rust_beatmap: Optional[Beatmap] = None
        self.rust_commit: str = ""
        self.beatmap_md5: str = ""
        self.warnings: List[Dict[str, Any]] = []
        self.unknown_fields: List[Dict[str, Any]] = []
        self.map_issues: List[Dict[str, Any]] = []
        self.replay_verification: Optional[ReplayVerificationResult] = None

    def __repr__(self):
        return f"<VerificationResult {os.path.basename(self.file_path)}: {self.status} ({self.reason})>"


def beatmap_from_rust_json(
    rust_data: Dict[str, Any], file_path: Optional[str] = None
) -> Beatmap:
    """
    Construct a full Beatmap object from rosea-app --json output.
    Audio fields are read from the .osu file because rosea-core omits audio by spec.
    """
    if rust_data.get("status") != "ok":
        raise ValueError(
            f"Cannot build Beatmap from Rust JSON: error status ({rust_data.get('error')})"
        )

    bm = Beatmap()
    bm.version = int(rust_data.get("version", 14))
    bm.mode = int(rust_data.get("mode", 0))
    bm.stack_leniency = float(rust_data.get("stack_leniency", 0.7))
    bm.beatmap_md5 = str(rust_data.get("beatmap_md5", ""))
    bm.rust_warnings = list(rust_data.get("warnings", []))

    # Audio fields only from .osu
    audio = read_osu_audio_fields(file_path or "")
    bm.audio_filename = audio["audio_filename"]
    bm.audio_lead_in = audio["audio_lead_in"]
    bm.preview_time = audio["preview_time"]

    # Metadata from Rust JSON
    r_meta = rust_data.get("metadata", {})
    bm.title = str(r_meta.get("title", ""))
    bm.title_unicode = r_meta.get("title_unicode")
    bm.artist = str(r_meta.get("artist", ""))
    bm.artist_unicode = r_meta.get("artist_unicode")
    bm.creator = str(r_meta.get("creator", ""))
    bm.version_name = str(r_meta.get("version", ""))
    bm.source = str(r_meta.get("source", ""))
    bm.tags = str(r_meta.get("tags", ""))
    bm.beatmap_id = r_meta.get("beatmap_id")
    bm.beatmapset_id = r_meta.get("beatmap_set_id")

    # Colours from Rust JSON
    r_col = rust_data.get("colours", {})
    combo_list = r_col.get("combo", [])
    if combo_list:
        bm.combo_colours = [
            (int(c["r"]), int(c["g"]), int(c["b"])) for c in combo_list
        ]
    bm.slider_track_override = _rgb_tuple(r_col.get("slider_track_override"))
    bm.slider_border = _rgb_tuple(r_col.get("slider_border"))

    # Break periods from Rust JSON
    bm.breaks = [
        (float(br["start_time"]), float(br["end_time"]))
        for br in rust_data.get("breaks", [])
    ]

    # Difficulty
    r_diff = rust_data.get("difficulty", {})
    bm.cs = float(r_diff.get("cs", 4.0))
    bm.od = float(r_diff.get("od", 8.0))
    bm.hp = float(r_diff.get("hp", 5.0))
    bm.ar = float(r_diff.get("ar", bm.od))
    sm_val = r_diff.get("slider_multiplier")
    bm.slider_multiplier = float(sm_val) if sm_val is not None else 1.4
    str_val = r_diff.get("slider_tick_rate")
    bm.slider_tick_rate = float(str_val) if str_val is not None else 1.0

    # Timing Points
    r_tp = rust_data.get("timing_points", [])
    bm.timing_points = []
    for tp_data in r_tp:
        tp = TimingPoint(
            line=int(tp_data.get("line", 0)),
            time=float(tp_data.get("time", 0.0)),
            beat_length=float(tp_data.get("beat_length", 500.0)),
            meter=int(tp_data.get("meter", 4)),
            sample_set=int(tp_data.get("sample_set", 1)),
            sample_index=int(tp_data.get("sample_index", 0)),
            volume=int(tp_data.get("volume", 70)),
            uninherited=bool(tp_data.get("uninherited", True)),
            effects=int(tp_data.get("effects", 0)),
        )
        bm.timing_points.append(tp)
    bm.timing_points.sort(key=lambda t: (t.time, t.line))

    # Hit Objects
    r_objs = rust_data.get("hit_objects", [])
    bm.hit_objects = []
    combo_idx = 0
    combo_num = 1

    for i, ro in enumerate(r_objs):
        obj_id = int(ro.get("id", i))
        line_num = int(ro.get("line", 0))
        x = float(ro.get("x", 0.0))
        y = float(ro.get("y", 0.0))
        t = float(ro.get("time", 0.0))
        flags = int(ro.get("flags", 1))
        hit_sound = int(ro.get("hit_sound", 0))
        kind = str(ro.get("kind", "circle"))

        obj = HitObject(obj_id, line_num, x, y, t, flags, hit_sound, kind)

        if kind == "slider":
            curve_str = str(ro.get("curve_kind", "linear"))
            if curve_str == "linear":
                curve_type = "L"
            elif curve_str == "bezier":
                curve_type = "B"
            elif curve_str in ("perfect", "perfect_curve"):
                curve_type = "P"
            else:
                curve_type = "L"
            controls = [(float(p["x"]), float(p["y"])) for p in ro.get("controls", [])]
            spans = int(ro.get("spans", 1))
            length = float(ro.get("declared_length", 0.0))

            obj.curve_type = curve_type
            obj.controls = controls
            obj.spans = spans
            obj.declared_length = length

            pts, cum_dist, tot_len = generate_slider_path(curve_type, controls, length)
            obj.path_points = pts
            obj.path_cum_dist = cum_dist
            obj.path_total_length = tot_len

            # Duration calculation
            uninherited_tp, active_tp = bm.get_timing_at(t)
            base_bl = uninherited_tp.beat_length if uninherited_tp else 500.0
            sv_mult = active_tp.sv_multiplier if active_tp else 1.0
            pixels_per_beat = bm.slider_multiplier * 100.0 * sv_mult
            velocity = pixels_per_beat / base_bl if base_bl > 0 else 1.0

            span_duration = (length / velocity) if velocity > 0 else 0.0
            total_duration = spans * span_duration
            obj.span_duration = span_duration
            obj.total_duration = total_duration
            obj.end_time = t + total_duration

            # Slider ticks
            tick_dist = (
                pixels_per_beat / bm.slider_tick_rate
                if bm.slider_tick_rate > 0
                else length
            )
            d = tick_dist
            ticks = []
            while d < length - 8.0:
                ticks.append(d)
                d += tick_dist
            obj.tick_distances = ticks
        elif kind == "spinner":
            end_t = float(ro.get("spinner_end_time", t))
            obj.spinner_end_time = end_t
            obj.end_time = end_t
        else:
            obj.end_time = t

        # Combo calculation
        if obj.is_new_combo or i == 0:
            combo_idx = (combo_idx + 1) % len(bm.combo_colours) if i > 0 else 0
            combo_num = 1
        obj.combo_index = combo_idx
        obj.combo_number = combo_num
        combo_num += 1

        bm.hit_objects.append(obj)

    # Sort objects by start time
    bm.hit_objects.sort(key=lambda o: (o.time, o.id))

    # Stacking
    preempt, _ = bm.ar_preempt_fadein
    stack_threshold = preempt * bm.stack_leniency
    apply_stacking(bm.hit_objects, stack_threshold, bm.circle_radius)

    return bm


def replay_from_rust_json(replay_data: Dict[str, Any]) -> Replay:
    """Construct a Replay object from Rust replay JSON (with normalized timeline frames)."""
    if replay_data.get("status") != "ok":
        raise ValueError(
            f"Cannot build Replay from Rust JSON: {replay_data.get('error_code')} ({replay_data.get('error')})"
        )

    hdr = replay_data.get("header", {})
    exp = replay_data.get("expected", {})
    frames = [
        ReplayFrame(
            time_ms=float(fr["time_ms"]),
            x=float(fr["x"]),
            y=float(fr["y"]),
            keys=int(fr["keys"]),
            index=int(fr["index"]),
        )
        for fr in replay_data.get("frames", [])
    ]

    return Replay(
        player=str(hdr.get("player", "")),
        frames=frames,
        score=int(exp.get("score", 0)),
        max_combo=int(exp.get("max_combo", 0)),
        count_300=int(exp.get("count_300", 0)),
        count_100=int(exp.get("count_100", 0)),
        count_50=int(exp.get("count_50", 0)),
        count_miss=int(exp.get("count_miss", 0)),
        mode=int(hdr.get("mode", 0)),
        game_version=int(hdr.get("game_version", 0)),
        beatmap_hash=str(hdr.get("beatmap_hash", "")),
        replay_hash=str(hdr.get("replay_hash", "")),
        mods=int(hdr.get("mods", 0)),
        perfect=int(hdr.get("perfect", 0)),
        count_geki=int(hdr.get("count_geki", 0)),
        count_katu=int(hdr.get("count_katu", 0)),
        seed=hdr.get("seed"),
        trailer_size=int(hdr.get("trailer_size", 0)),
        raw_record_count=int(hdr.get("raw_record_count", len(frames))),
        source_engine="rust",
        normalization=replay_data.get("normalization"),
        warnings=list(replay_data.get("warnings", [])),
    )


def verify_replay_from_json(
    beatmap_path: str,
    replay_path: str,
    py_bm: Optional[Beatmap],
    rust_out: Dict[str, Any],
) -> ReplayVerificationResult:
    """Verify Rust replay parsing, beatmap MD5 match, and normalization against Python."""
    res = ReplayVerificationResult(beatmap_path, replay_path)
    py_rep = parse_osr_replay(replay_path)
    res.py_replay = py_rep

    r_rep = rust_out.get("replay")
    res.rust_replay_data = r_rep
    if not r_rep:
        res.status = "FAIL"
        res.reason = "Rust output did not include replay object"
        return res

    res.error_code = r_rep.get("error_code")
    res.error_location = r_rep.get("error_location")
    res.hash_status = str(r_rep.get("hash_status", "unchecked"))
    res.hash_match = r_rep.get("hash_match")
    res.warnings = list(r_rep.get("warnings", []))
    res.normalization = r_rep.get("normalization")

    hdr = r_rep.get("header") or {}
    res.trailer_offset = int(hdr.get("trailer_offset", 0))
    res.trailer_hex = str(hdr.get("trailer_hex", ""))
    res.trailer_ascii = str(hdr.get("trailer_ascii", ""))
    res.trailer_i64_le = hdr.get("trailer_i64_le")

    def _finalize(result: ReplayVerificationResult) -> ReplayVerificationResult:
        result.replay_issues = build_replay_issues(
            py_rep,
            result.rust_replay,
            r_rep,
            result.mismatches,
            result.error_code,
            result.error_location,
        )
        return result

    # Compute raw map MD5 independently in Python if py_bm doesn't have it
    py_md5 = py_bm.beatmap_md5 if (py_bm and py_bm.beatmap_md5) else ""
    if not py_md5 and os.path.isfile(beatmap_path):
        with open(beatmap_path, "rb") as f:
            py_md5 = hashlib.md5(f.read()).hexdigest()

    if r_rep.get("status") != "ok":
        err_code = res.error_code or "UnknownError"
        err_msg = r_rep.get("error", "")

        if err_code == "BeatmapHashMismatch":
            if py_rep and py_md5 and py_rep.beatmap_hash.lower() == py_md5.lower():
                res.status = "FAIL"
                res.reason = f"Rust reported BeatmapHashMismatch, but Python MD5 matches ({py_md5})"
            else:
                res.status = "HASH_MISMATCH"
                rep_h = py_rep.beatmap_hash if py_rep else hdr.get("beatmap_hash", "?")
                res.reason = f"BeatmapHashMismatch (map={py_md5[:8]}..., replay={str(rep_h)[:8]}...)"
            return _finalize(res)

        if err_code in (
            "UnsupportedMods",
            "UnsupportedReplayMode",
            "NoReplayData",
            "UnsupportedReplayFeature",
        ):
            if err_code == "UnsupportedMods" and py_rep and py_rep.mods == 0:
                res.status = "FAIL"
                res.reason = "Rust reported UnsupportedMods for mods=0"
                return _finalize(res)
            if err_code == "UnsupportedReplayMode" and py_rep and py_rep.mode == 0:
                res.status = "FAIL"
                res.reason = "Rust reported UnsupportedReplayMode for mode=0"
                return _finalize(res)
            res.status = "SKIPPED_UNSUPPORTED"
            res.reason = f"Rust rejected replay ({err_code}): {err_msg}"
            return _finalize(res)

        res.status = "FAIL"
        res.reason = f"Rust replay failed ({err_code}): {err_msg}"
        return _finalize(res)

    # Status == "ok": compare Rust replay header, expected stats, MD5, and normalized count
    if py_rep is None:
        res.status = "FAIL"
        res.reason = "Python parse_osr_replay failed on replay that Rust accepted"
        return _finalize(res)

    if res.hash_match is not True or res.hash_status != "ok":
        res.mismatches.append(f"Expected hash_match=True/ok, got {res.hash_match}/{res.hash_status}")

    if py_md5 and py_rep.beatmap_hash.lower() != py_md5.lower():
        res.mismatches.append(
            f"MD5 mismatch bypassed Rust check: map={py_md5}, replay={py_rep.beatmap_hash}"
        )

    exp = r_rep.get("expected", {})

    if hdr.get("mode") != py_rep.mode:
        res.mismatches.append(f"mode mismatch: Rust={hdr.get('mode')}, Py={py_rep.mode}")
    if hdr.get("player") != py_rep.player:
        res.mismatches.append(f"Player mismatch: Rust={hdr.get('player')!r}, Py={py_rep.player!r}")
    if hdr.get("game_version") != py_rep.game_version:
        res.mismatches.append(
            f"game_version mismatch: Rust={hdr.get('game_version')}, Py={py_rep.game_version}"
        )
    if hdr.get("replay_hash") != py_rep.replay_hash:
        res.mismatches.append(
            f"replay_hash mismatch: Rust={hdr.get('replay_hash')!r}, Py={py_rep.replay_hash!r}"
        )
    if hdr.get("mods") != py_rep.mods:
        res.mismatches.append(f"mods mismatch: Rust={hdr.get('mods')}, Py={py_rep.mods}")
    if hdr.get("perfect") != py_rep.perfect:
        res.mismatches.append(f"perfect mismatch: Rust={hdr.get('perfect')}, Py={py_rep.perfect}")
    if hdr.get("count_geki") != py_rep.count_geki:
        res.mismatches.append(
            f"count_geki mismatch: Rust={hdr.get('count_geki')}, Py={py_rep.count_geki}"
        )
    if hdr.get("count_katu") != py_rep.count_katu:
        res.mismatches.append(
            f"count_katu mismatch: Rust={hdr.get('count_katu')}, Py={py_rep.count_katu}"
        )
    if hdr.get("seed") != py_rep.seed:
        res.mismatches.append(f"seed mismatch: Rust={hdr.get('seed')}, Py={py_rep.seed}")
    if hdr.get("trailer_size") != py_rep.trailer_size:
        res.mismatches.append(
            f"trailer_size mismatch: Rust={hdr.get('trailer_size')}, Py={py_rep.trailer_size}"
        )
    if hdr.get("raw_record_count") != py_rep.raw_record_count:
        res.mismatches.append(
            f"raw_record_count mismatch: Rust={hdr.get('raw_record_count')}, Py={py_rep.raw_record_count}"
        )

    for r_key, py_attr in [
        ("score", "score"),
        ("max_combo", "max_combo"),
        ("count_300", "count_300"),
        ("count_100", "count_100"),
        ("count_50", "count_50"),
        ("count_miss", "count_miss"),
    ]:
        py_val = getattr(py_rep, py_attr)
        if exp.get(r_key) != py_val:
            res.mismatches.append(f"Replay {r_key} mismatch: Rust={exp.get(r_key)}, Py={py_val}")

    norm = res.normalization or {}
    removed = int(norm.get("initial_dummy_removed", 0)) + int(norm.get("backward_removed", 0))
    expected_norm_count = len(py_rep.frames) - removed
    r_frames = r_rep.get("frames", [])
    actual_norm_count = int(r_rep.get("record_count", len(r_frames)))
    if len(r_frames) != actual_norm_count:
        res.mismatches.append(
            f"Replay frames length mismatch: record_count={actual_norm_count}, len(frames)={len(r_frames)}"
        )
    if actual_norm_count != expected_norm_count:
        res.mismatches.append(
            f"Normalized frame count mismatch: Rust={actual_norm_count}, Py={len(py_rep.frames)} - {removed}"
        )
    for fi in range(1, len(r_frames)):
        if r_frames[fi]["time_ms"] < r_frames[fi - 1]["time_ms"]:
            res.mismatches.append(
                f"Normalized frame time went backwards at [{fi}]: {r_frames[fi]['time_ms']} < {r_frames[fi - 1]['time_ms']}"
            )
            break

    if res.mismatches:
        res.status = "FAIL"
        res.reason = "; ".join(res.mismatches[:3])
        return _finalize(res)

    try:
        res.rust_replay = replay_from_rust_json(r_rep)
    except Exception as e:
        res.status = "FAIL"
        res.mismatches.append(str(e))
        res.reason = f"Failed to build Replay from Rust JSON: {e}"
        return _finalize(res)

    res.status = "PASS"
    res.reason = (
        f"Verified {actual_norm_count} frames, MD5 ok, "
        f"norm(t={norm.get('changed_times', 0)}, dummy={norm.get('initial_dummy_removed', 0)}, "
        f"back={norm.get('backward_removed', 0)})"
    )
    return _finalize(res)


def verify_replay(beatmap_path: str, replay_path: str) -> ReplayVerificationResult:
    """Verify a beatmap + replay pair directly through Rust CLI and Python parser."""
    py_bm = parse_osu_beatmap(beatmap_path)
    rust_out = run_rust_parser(beatmap_path, replay_path=replay_path)
    return verify_replay_from_json(beatmap_path, replay_path, py_bm, rust_out)


def verify_beatmap(
    file_path: str, replay_path: Optional[str] = None
) -> VerificationResult:
    """
    Compare Python parser and Rust parser on the given beatmap (and optional replay) file.
    """
    res = VerificationResult(file_path)

    try:
        py_bm = parse_osu_beatmap(file_path)
        res.py_beatmap = py_bm
    except Exception as e:
        res.status = "FAIL"
        res.reason = f"Python parser failed: {e}"
        res.unknown_fields = scan_unknown_beatmap_fields(file_path)
        res.map_issues = build_map_issues(file_path, None, None, res.mismatches)
        return res

    # Determine what kinds of objects this map has
    kinds = set()
    for obj in py_bm.hit_objects:
        if obj.kind == "slider":
            kinds.add(f"slider_{obj.curve_type}")
        else:
            kinds.add(obj.kind)

    has_unsupported_raw = bool(getattr(py_bm, "unsupported_objects", []))

    # Rust supports v14 osu!standard with Circle, Linear (L), Bezier (B), Perfect Curve (P), and Spinner
    rust_supported = (
        py_bm.version == 14
        and py_bm.mode == 0
        and not has_unsupported_raw
        and all(k in ("circle", "slider_L", "slider_B", "slider_P", "spinner") for k in kinds)
        and len(py_bm.hit_objects) > 0
    )
    res.rust_supported = rust_supported

    rust_out = run_rust_parser(file_path, replay_path=replay_path)
    res.rust_data = rust_out
    res.rust_commit = rust_out.get("commit", get_rust_commit_hash())
    res.beatmap_md5 = str(rust_out.get("beatmap_md5", py_bm.beatmap_md5))
    res.warnings = list(rust_out.get("warnings", []))
    res.error_code = rust_out.get("error_code")
    res.error_location = rust_out.get("error_location")
    if isinstance(rust_out.get("unknown_fields"), list):
        res.unknown_fields = list(rust_out["unknown_fields"])
    else:
        res.unknown_fields = scan_unknown_beatmap_fields(file_path)

    if replay_path and rust_out.get("replay") is not None:
        res.replay_verification = verify_replay_from_json(
            file_path, replay_path, py_bm, rust_out
        )

    def _finalize_map(result: VerificationResult) -> VerificationResult:
        result.map_issues = build_map_issues(file_path, py_bm, rust_out, result.mismatches)
        return result

    if not rust_supported:
        if rust_out.get("status") == "error":
            unsupported_kinds = [
                k for k in kinds if k not in ("circle", "slider_L", "slider_B", "slider_P", "spinner")
            ]
            if py_bm.version != 14:
                unsupported_kinds.append(f"v{py_bm.version}")
            if py_bm.mode != 0:
                unsupported_kinds.append(f"mode_{py_bm.mode}")
            if has_unsupported_raw and not unsupported_kinds:
                unsupported_kinds.append("unsupported_flags_or_malformed")
            res.status = "SKIPPED_UNSUPPORTED"
            res.reason = (
                f"Map contains unsupported objects ({', '.join(sorted(unsupported_kinds))}). "
                f"Rust correctly rejected: {rust_out.get('error')}"
            )
            return _finalize_map(res)
        else:
            res.status = "FAIL"
            res.reason = (
                "Map contains unsupported objects in current Rust parser, "
                "but Rust parser returned status 'ok' unexpectedly."
            )
            return _finalize_map(res)

    if rust_out.get("status") != "ok":
        res.status = "FAIL"
        res.reason = f"Rust parser failed unexpectedly: {rust_out.get('error')}"
        return _finalize_map(res)

    # Verify header, mode, MD5, stack_leniency
    if rust_out.get("version") != py_bm.version:
        res.mismatches.append(
            f"Version mismatch: Rust={rust_out.get('version')}, Python={py_bm.version}"
        )
    if rust_out.get("mode") != py_bm.mode:
        res.mismatches.append(
            f"Mode mismatch: Rust={rust_out.get('mode')}, Python={py_bm.mode}"
        )
    if rust_out.get("beatmap_md5") != py_bm.beatmap_md5:
        res.mismatches.append(
            f"MD5 mismatch: Rust={rust_out.get('beatmap_md5')}, Python={py_bm.beatmap_md5}"
        )
    r_sl = rust_out.get("stack_leniency")
    if r_sl is None or abs(float(r_sl) - float(py_bm.stack_leniency)) > 1e-4:
        res.mismatches.append(
            f"StackLeniency mismatch: Rust={r_sl}, Python={py_bm.stack_leniency}"
        )

    # Verify Metadata
    r_meta = rust_out.get("metadata", {})
    for r_key, py_attr in [
        ("title", "title"),
        ("title_unicode", "title_unicode"),
        ("artist", "artist"),
        ("artist_unicode", "artist_unicode"),
        ("creator", "creator"),
        ("version", "version_name"),
        ("source", "source"),
        ("tags", "tags"),
        ("beatmap_id", "beatmap_id"),
        ("beatmap_set_id", "beatmapset_id"),
    ]:
        r_val = r_meta.get(r_key)
        py_val = getattr(py_bm, py_attr)
        if r_val != py_val:
            res.mismatches.append(
                f"Metadata '{r_key}' mismatch: Rust={r_val!r}, Python={py_val!r}"
            )

    # Verify Colours
    r_col = rust_out.get("colours", {})
    r_combo = [
        (int(c["r"]), int(c["g"]), int(c["b"])) for c in r_col.get("combo", [])
    ]
    if r_combo != py_bm.combo_colours:
        res.mismatches.append(
            f"Colours combo mismatch: Rust={r_combo}, Python={py_bm.combo_colours}"
        )
    r_sto = _rgb_tuple(r_col.get("slider_track_override"))
    if r_sto != py_bm.slider_track_override:
        res.mismatches.append(
            f"SliderTrackOverride mismatch: Rust={r_sto}, Python={py_bm.slider_track_override}"
        )
    r_sb = _rgb_tuple(r_col.get("slider_border"))
    if r_sb != py_bm.slider_border:
        res.mismatches.append(
            f"SliderBorder mismatch: Rust={r_sb}, Python={py_bm.slider_border}"
        )

    # Verify Breaks
    r_breaks = rust_out.get("breaks", [])
    if len(r_breaks) != len(py_bm.breaks):
        res.mismatches.append(
            f"Breaks count mismatch: Rust={len(r_breaks)}, Python={len(py_bm.breaks)}"
        )
    else:
        for i, (rb, (ps, pe)) in enumerate(zip(r_breaks, py_bm.breaks)):
            if abs(float(rb["start_time"]) - ps) > 1e-4 or abs(float(rb["end_time"]) - pe) > 1e-4:
                res.mismatches.append(
                    f"Break[{i}] mismatch: Rust=({rb['start_time']},{rb['end_time']}), Py=({ps},{pe})"
                )
                break

    # Verify difficulty
    r_diff = rust_out.get("difficulty", {})
    for field, py_val in [
        ("cs", py_bm.cs),
        ("od", py_bm.od),
        ("hp", py_bm.hp),
        ("ar", py_bm.ar),
    ]:
        r_val = r_diff.get(field)
        if r_val is None:
            res.mismatches.append(f"Difficulty field '{field}' missing in Rust output")
        elif abs(float(r_val) - float(py_val)) > 1e-4:
            res.mismatches.append(
                f"Difficulty '{field}' mismatch: Rust={r_val}, Python={py_val}"
            )

    has_sliders = any(obj.kind == "slider" for obj in py_bm.hit_objects)
    if has_sliders:
        for field, py_val in [
            ("slider_multiplier", py_bm.slider_multiplier),
            ("slider_tick_rate", py_bm.slider_tick_rate),
        ]:
            r_val = r_diff.get(field)
            if r_val is None:
                res.mismatches.append(
                    f"Difficulty field '{field}' missing in Rust output"
                )
            elif abs(float(r_val) - float(py_val)) > 1e-4:
                res.mismatches.append(
                    f"Difficulty '{field}' mismatch: Rust={r_val}, Python={py_val}"
                )
    else:
        for field in ("slider_multiplier", "slider_tick_rate"):
            r_val = r_diff.get(field)
            if r_val is not None:
                py_val = getattr(py_bm, field)
                if abs(float(r_val) - float(py_val)) > 1e-4:
                    res.mismatches.append(
                        f"Difficulty '{field}' mismatch: Rust={r_val}, Python={py_val}"
                    )

    # Verify timing points count
    r_tp = rust_out.get("timing_points", [])
    if len(r_tp) != len(py_bm.timing_points):
        res.mismatches.append(
            f"Timing points count mismatch: Rust={len(r_tp)}, Python={len(py_bm.timing_points)}"
        )
    else:
        for i, (rt, pt) in enumerate(zip(r_tp, py_bm.timing_points)):
            if abs(rt["time"] - pt.time) > 1e-4:
                res.mismatches.append(
                    f"TP[{i}] time mismatch: Rust={rt['time']}, Py={pt.time}"
                )
                break
            if abs(rt["beat_length"] - pt.beat_length) > 1e-4:
                res.mismatches.append(
                    f"TP[{i}] beat_length mismatch: Rust={rt['beat_length']}, Py={pt.beat_length}"
                )
                break
            if rt["uninherited"] != pt.uninherited:
                res.mismatches.append(
                    f"TP[{i}] uninherited mismatch: Rust={rt['uninherited']}, Py={pt.uninherited}"
                )
                break

    # Verify hit objects
    r_objs = rust_out.get("hit_objects", [])
    if len(r_objs) != len(py_bm.hit_objects):
        res.mismatches.append(
            f"Hit objects count mismatch: Rust={len(r_objs)}, Python={len(py_bm.hit_objects)}"
        )
    else:
        for i, (ro, po) in enumerate(zip(r_objs, py_bm.hit_objects)):
            res.objects_checked += 1
            if ro["kind"] != po.kind:
                res.mismatches.append(
                    f"Obj[{i}] kind mismatch: Rust={ro['kind']}, Py={po.kind}"
                )
                break
            if abs(ro["time"] - po.time) > 1e-4:
                res.mismatches.append(
                    f"Obj[{i}] time mismatch: Rust={ro['time']}, Py={po.time}"
                )
                break
            if abs(ro["x"] - po.x) > 1e-3 or abs(ro["y"] - po.y) > 1e-3:
                res.mismatches.append(
                    f"Obj[{i}] pos mismatch: Rust=({ro['x']},{ro['y']}), Py=({po.x},{po.y})"
                )
                break
            if ro.get("flags") != po.flags:
                res.mismatches.append(
                    f"Obj[{i}] flags mismatch: Rust={ro.get('flags')}, Py={po.flags}"
                )
                break
            if ro["kind"] == "slider":
                curve_str = str(ro.get("curve_kind", "linear"))
                expected_type = {"linear": "L", "bezier": "B", "perfect": "P"}.get(curve_str, "")
                if expected_type != po.curve_type:
                    res.mismatches.append(
                        f"Obj[{i}] slider curve mismatch: Rust={curve_str}, Py={po.curve_type}"
                    )
                    break
                if ro.get("spans") != po.spans:
                    res.mismatches.append(
                        f"Obj[{i}] spans mismatch: Rust={ro.get('spans')}, Py={po.spans}"
                    )
                    break
                if abs(ro.get("declared_length", 0.0) - po.declared_length) > 1e-3:
                    res.mismatches.append(
                        f"Obj[{i}] length mismatch: Rust={ro.get('declared_length')}, Py={po.declared_length}"
                    )
                    break
                r_ctrls = ro.get("controls", [])
                if len(r_ctrls) != len(po.controls):
                    res.mismatches.append(
                        f"Obj[{i}] controls count mismatch: Rust={len(r_ctrls)}, Py={len(po.controls)}"
                    )
                    break
                for ci, (rc, pc) in enumerate(zip(r_ctrls, po.controls)):
                    if abs(rc["x"] - pc[0]) > 1e-3 or abs(rc["y"] - pc[1]) > 1e-3:
                        res.mismatches.append(
                            f"Obj[{i}] control[{ci}] mismatch: Rust=({rc['x']},{rc['y']}), Py={pc}"
                        )
                        break
            elif ro["kind"] == "spinner":
                if po.kind != "spinner":
                    res.mismatches.append(f"Obj[{i}] kind mismatch: Rust=spinner, Py={po.kind}")
                    break
                r_end = float(ro.get("spinner_end_time", ro.get("end_time", 0.0)))
                p_end = getattr(po, "spinner_end_time", getattr(po, "end_time", 0.0))
                if abs(r_end - p_end) > 1e-3:
                    res.mismatches.append(
                        f"Obj[{i}] spinner end mismatch: Rust={r_end}, Py={p_end}"
                    )
                    break

    if res.mismatches:
        res.status = "FAIL"
        res.reason = f"{len(res.mismatches)} mismatches found: " + "; ".join(
            res.mismatches[:3]
        )
    else:
        res.status = "PASS"
        res.reason = (
            f"100% match across {res.objects_checked} objects, "
            f"{len(py_bm.timing_points)} timing points, {len(py_bm.breaks)} breaks!"
        )
        try:
            res.rust_beatmap = beatmap_from_rust_json(rust_out, file_path)
        except Exception as e:
            res.mismatches.append(f"Failed to build Beatmap from Rust JSON: {e}")
            res.status = "FAIL"

    return _finalize_map(res)


def run_full_corpus_verification(
    corpus_dir: str = "tests/data", verify_replays: bool = True
) -> Dict[str, Any]:
    """Run verification across all .osu (and paired .osr) files in corpus."""
    import glob

    pattern = os.path.join(corpus_dir, "**", "*.osu")
    files = sorted(glob.glob(pattern, recursive=True))

    passed = []
    skipped = []
    failed = []

    for f in files:
        candidate_osr = os.path.splitext(f)[0] + ".osr"
        rep = candidate_osr if (verify_replays and os.path.isfile(candidate_osr)) else None
        res = verify_beatmap(f, replay_path=rep)
        if res.status == "PASS":
            if res.replay_verification and res.replay_verification.status == "FAIL":
                res.status = "FAIL"
                res.reason = f"Replay verification failed: {res.replay_verification.reason}"
                failed.append(res)
            else:
                passed.append(res)
        elif res.status == "SKIPPED_UNSUPPORTED":
            skipped.append(res)
        else:
            failed.append(res)

    return {
        "total": len(files),
        "passed": passed,
        "skipped": skipped,
        "failed": failed,
    }


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Verify rosea Rust parser against beatmaps")
    parser.add_argument(
        "path",
        nargs="?",
        default="tests/data/01_micro/2dearbrave.osu",
        help="Beatmap file to verify",
    )
    parser.add_argument("--replay", type=str, default=None, help="Optional .osr replay file")
    parser.add_argument("--all", action="store_true", help="Verify all beatmaps in tests/data")
    args = parser.parse_args()

    if args.all:
        report = run_full_corpus_verification()
        print(f"=== Verification Report: {report['total']} maps ===")
        print(f"Passed (fully verified with Rust): {len(report['passed'])}")
        for r in report["passed"]:
            rep_note = (
                f" | Replay: {r.replay_verification.status}"
                if r.replay_verification
                else ""
            )
            print(f"  [PASS] {os.path.basename(r.file_path)}: {r.reason}{rep_note}")
        print(
            f"\nSkipped (correctly rejected by Rust due to unsupported curves/objects): {len(report['skipped'])}"
        )
        for r in report["skipped"]:
            print(f"  [SKIP] {os.path.basename(r.file_path)}: {r.reason}")
        print(f"\nFailed: {len(report['failed'])}")
        for r in report["failed"]:
            print(f"  [FAIL] {os.path.basename(r.file_path)}: {r.reason}")
        if report["failed"]:
            sys.exit(1)
    else:
        res = verify_beatmap(args.path, replay_path=args.replay)
        print(f"Verification of {args.path}:")
        print(f"  Status: {res.status}")
        print(f"  MD5:    {res.beatmap_md5}")
        print(f"  Reason: {res.reason}")
        if res.warnings:
            print(f"  Warnings: {res.warnings}")
        if res.replay_verification:
            rv = res.replay_verification
            print(f"  Replay Status: {rv.status} ({rv.reason})")
            if rv.warnings:
                print(f"  Replay Warnings: {rv.warnings}")
        if res.mismatches:
            print("  Mismatches:")
            for m in res.mismatches:
                print(f"    - {m}")
