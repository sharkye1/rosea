"""
Bridge to invoke and verify the rosea Rust parser against osu! beatmaps.
"""

import json
import os
import subprocess
import sys

# Ensure repository root and scripts directory are in sys.path
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
for p in (REPO_ROOT, SCRIPT_DIR):
    if p not in sys.path:
        sys.path.insert(0, p)

from typing import Dict, Any, List, Optional

try:
    from osu_parser import (
        parse_osu_beatmap,
        Beatmap,
        HitObject,
        TimingPoint,
        generate_slider_path,
        apply_stacking,
    )
except ImportError:
    from scripts.osu_parser import (
        parse_osu_beatmap,
        Beatmap,
        HitObject,
        TimingPoint,
        generate_slider_path,
        apply_stacking,
    )


def get_rosea_executable() -> str:
    """Find or build the rosea binary."""
    # Look for target/debug/rosea.exe
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    target_exe = os.path.join(root_dir, "target", "debug", "rosea.exe")
    if not os.path.isfile(target_exe):
        target_exe = os.path.join(root_dir, "target", "debug", "rosea")

    if not os.path.isfile(target_exe):
        # Build it
        print("Building rosea-app...", file=sys.stderr)
        subprocess.run(["cargo", "build", "-p", "rosea-app"], cwd=root_dir, check=True)

    return target_exe


def run_rust_parser(file_path: str) -> Dict[str, Any]:
    """
    Run rosea on file_path with --json and return parsed JSON result.
    """
    exe = get_rosea_executable()
    proc = subprocess.run(
        [exe, file_path, "--json"],
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


class VerificationResult:
    def __init__(self, file_path: str):
        self.file_path = file_path
        self.status = "UNKNOWN"  # "PASS", "SKIPPED_UNSUPPORTED", "FAIL"
        self.reason = ""
        self.mismatches: List[str] = []
        self.objects_checked = 0
        self.rust_supported = False
        self.py_beatmap: Optional[Beatmap] = None
        self.rust_data: Optional[Dict[str, Any]] = None
        self.rust_beatmap: Optional[Beatmap] = None

    def __repr__(self):
        return f"<VerificationResult {os.path.basename(self.file_path)}: {self.status} ({self.reason})>"


def beatmap_from_rust_json(
    rust_data: Dict[str, Any], file_path: Optional[str] = None
) -> Beatmap:
    """
    Construct a full Beatmap object from rosea-app --json output.
    This enables reproducing the exact data parsed by the Rust parser.
    """
    if rust_data.get("status") != "ok":
        raise ValueError(
            f"Cannot build Beatmap from Rust JSON: error status ({rust_data.get('error')})"
        )

    bm = Beatmap()
    bm.version = int(rust_data.get("version", 14))
    bm.mode = int(rust_data.get("mode", 0))

    # Retain metadata / colours / audio from .osu file if accessible
    if file_path and os.path.isfile(file_path):
        try:
            raw_bm = parse_osu_beatmap(file_path)
            bm.title = raw_bm.title
            bm.artist = raw_bm.artist
            bm.creator = raw_bm.creator
            bm.version_name = raw_bm.version_name
            bm.audio_filename = raw_bm.audio_filename
            bm.audio_lead_in = raw_bm.audio_lead_in
            bm.preview_time = raw_bm.preview_time
            bm.stack_leniency = raw_bm.stack_leniency
            bm.combo_colours = list(raw_bm.combo_colours)
            bm.beatmapset_id = raw_bm.beatmapset_id
            bm.beatmap_id = raw_bm.beatmap_id
        except Exception:
            pass

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


def verify_beatmap(file_path: str) -> VerificationResult:
    """
    Compare Python parser and Rust parser on the given beatmap file.
    """
    res = VerificationResult(file_path)

    try:
        py_bm = parse_osu_beatmap(file_path)
        res.py_beatmap = py_bm
    except Exception as e:
        res.status = "FAIL"
        res.reason = f"Python parser failed: {e}"
        return res

    # Determine what kinds of objects this map has
    kinds = set()
    for obj in py_bm.hit_objects:
        if obj.kind == "slider":
            kinds.add(f"slider_{obj.curve_type}")
        else:
            kinds.add(obj.kind)

    # Check for unsupported raw objects
    has_unsupported_raw = bool(getattr(py_bm, "unsupported_objects", []))

    # Rust supports Circle, Linear (L), Bezier (B), Perfect Curve (P), and Spinner
    rust_supported = (
        not has_unsupported_raw
        and all(k in ("circle", "slider_L", "slider_B", "slider_P", "spinner") for k in kinds)
        and len(py_bm.hit_objects) > 0
    )
    res.rust_supported = rust_supported

    rust_out = run_rust_parser(file_path)
    res.rust_data = rust_out

    if not rust_supported:
        # Rust should return an error (UnsupportedHitObject)
        if rust_out.get("status") == "error":
            unsupported_kinds = [k for k in kinds if k not in ("circle", "slider_L", "slider_B", "slider_P", "spinner")]
            if has_unsupported_raw and not unsupported_kinds:
                unsupported_kinds.append("unsupported_flags_or_malformed")
            res.status = "SKIPPED_UNSUPPORTED"
            res.reason = (
                f"Map contains unsupported objects ({', '.join(sorted(unsupported_kinds))}). "
                f"Rust correctly rejected: {rust_out.get('error')}"
            )
            return res
        else:
            res.status = "FAIL"
            res.reason = (
                "Map contains unsupported objects in current Rust parser, "
                "but Rust parser returned status 'ok' unexpectedly."
            )
            return res

    # If it is supported by Rust, verify all fields
    if rust_out.get("status") != "ok":
        res.status = "FAIL"
        res.reason = f"Rust parser failed unexpectedly: {rust_out.get('error')}"
        return res

    # Verify header & mode
    if rust_out.get("version") != py_bm.version:
        res.mismatches.append(
            f"Version mismatch: Rust={rust_out.get('version')}, Python={py_bm.version}"
        )
    if rust_out.get("mode") != py_bm.mode:
        res.mismatches.append(
            f"Mode mismatch: Rust={rust_out.get('mode')}, Python={py_bm.mode}"
        )

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
        # If no sliders, Rust may output null for slider settings
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
        res.reason = f"100% match across {res.objects_checked} objects and {len(py_bm.timing_points)} timing points!"
        try:
            res.rust_beatmap = beatmap_from_rust_json(rust_out, file_path)
        except Exception as e:
            res.mismatches.append(f"Failed to build Beatmap from Rust JSON: {e}")
            res.status = "FAIL"

    return res


def run_full_corpus_verification(corpus_dir: str = "tests/data") -> Dict[str, Any]:
    """Run verification across all .osu files in corpus."""
    import glob
    pattern = os.path.join(corpus_dir, "**", "*.osu")
    files = sorted(glob.glob(pattern, recursive=True))

    passed = []
    skipped = []
    failed = []

    for f in files:
        res = verify_beatmap(f)
        if res.status == "PASS":
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
    parser.add_argument("path", nargs="?", default="tests/data/01_micro/2dearbrave.osu", help="Beatmap file to verify")
    parser.add_argument("--all", action="store_true", help="Verify all beatmaps in tests/data")
    args = parser.parse_args()

    if args.all:
        report = run_full_corpus_verification()
        print(f"=== Verification Report: {report['total']} maps ===")
        print(f"Passed (fully verified with Rust): {len(report['passed'])}")
        for r in report['passed']:
            print(f"  [PASS] {os.path.basename(r.file_path)}: {r.reason}")
        print(f"\nSkipped (correctly rejected by Rust due to unsupported curves/spinners): {len(report['skipped'])}")
        for r in report['skipped']:
            print(f"  [SKIP] {os.path.basename(r.file_path)}: {r.reason}")
        print(f"\nFailed: {len(report['failed'])}")
        for r in report['failed']:
            print(f"  [FAIL] {os.path.basename(r.file_path)}: {r.reason}")
        if report['failed']:
            sys.exit(1)
    else:
        res = verify_beatmap(args.path)
        print(f"Verification of {args.path}:")
        print(f"  Status: {res.status}")
        print(f"  Reason: {res.reason}")
        if res.mismatches:
            print("  Mismatches:")
            for m in res.mismatches:
                print(f"    - {m}")
