"""
Automated unit and integration test suite for osu_parser, rust_bridge, and visualizer.
Covers:
- All curve types: Linear (L), Bézier (B), Circular Arc (P, including collinear), Catmull-Rom (C)
- Slider length truncation, zero length, and extension along final tangent
- AR fallback to OD when ApproachRate is omitted
- Circle-only maps verification without SliderMultiplier
- Live engine source switching (Rust <-> Python)
- Replay (.osr) loading and cursor interpolation
- Full corpus verification across tests/data
"""

import math
import os
import sys
import tempfile

# Setup paths
SCRIPT_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "scripts")
sys.path.insert(0, SCRIPT_DIR)

from osu_parser import (
    generate_bezier_curve,
    generate_circular_arc,
    generate_catmull_curve,
    generate_slider_path,
    parse_osu_beatmap,
    parse_osr_replay,
    distance,
)
from rust_bridge import (
    verify_beatmap,
    run_full_corpus_verification,
    run_rust_parser,
    beatmap_from_rust_json,
)
from visualizer import Visualizer


def test_linear_curve():
    controls = [(0.0, 0.0), (100.0, 0.0)]
    pts, cum_dist, tot_len = generate_slider_path("L", controls, 100.0)
    assert abs(tot_len - 100.0) < 1e-3, f"Expected 100.0, got {tot_len}"
    assert abs(pts[-1][0] - 100.0) < 1e-3
    assert abs(pts[-1][1] - 0.0) < 1e-3
    print("test_linear_curve: OK")


def test_bezier_multi_segment():
    # Consecutive duplicate points split Bézier into multi-segments
    controls = [(0.0, 0.0), (50.0, 50.0), (50.0, 50.0), (100.0, 0.0)]
    pts, cum_dist, tot_len = generate_slider_path("B", controls, 141.421356)
    assert len(pts) > 2
    assert abs(pts[0][0] - 0.0) < 1e-3
    assert abs(pts[-1][0] - 100.0) < 1e-3
    assert abs(pts[-1][1] - 0.0) < 1e-3
    print("test_bezier_multi_segment: OK")


def test_circular_arc_normal_and_collinear():
    # Normal arc passing through 3 points
    a = (0.0, 0.0)
    b = (50.0, 50.0)
    c = (100.0, 0.0)
    pts = generate_circular_arc(a, b, c)
    assert len(pts) > 5
    assert abs(pts[0][0] - 0.0) < 1e-3
    assert abs(pts[-1][0] - 100.0) < 1e-3

    # Collinear fallback: straight lines reaching middle control point even when non-monotonic
    a = (0.0, 0.0)
    b = (100.0, 0.0)
    c = (50.0, 0.0)
    pts_collinear = generate_circular_arc(a, b, c)
    max_x = max(p[0] for p in pts_collinear)
    assert abs(max_x - 100.0) < 1e-3, f"Collinear arc must reach control point b (100.0), reached {max_x}"
    assert abs(pts_collinear[-1][0] - 50.0) < 1e-3
    print("test_circular_arc_normal_and_collinear: OK")


def test_catmull_curve():
    controls = [(0.0, 0.0), (30.0, 60.0), (70.0, 20.0), (100.0, 80.0)]
    pts = generate_catmull_curve(controls)
    assert len(pts) > 10
    assert abs(pts[0][0] - controls[0][0]) < 1e-3
    assert abs(pts[-1][0] - controls[-1][0]) < 1e-3
    print("test_catmull_curve: OK")


def test_slider_truncation_and_extension():
    # 1. Truncation when declared length is shorter than control polygon
    controls = [(0.0, 0.0), (200.0, 0.0)]
    pts, cum_dist, tot_len = generate_slider_path("L", controls, 75.0)
    assert abs(tot_len - 75.0) < 1e-3
    assert abs(pts[-1][0] - 75.0) < 1e-3

    # 2. Extension along final tangent when declared length exceeds control polygon
    controls = [(0.0, 0.0), (100.0, 0.0)]
    pts_ext, cum_dist_ext, tot_len_ext = generate_slider_path("L", controls, 180.0)
    assert abs(tot_len_ext - 180.0) < 1e-3
    assert abs(pts_ext[-1][0] - 180.0) < 1e-3

    # 3. Degenerate zero length slider
    controls = [(10.0, 20.0), (50.0, 20.0)]
    pts_zero, cum_dist_zero, tot_len_zero = generate_slider_path("L", controls, 0.0)
    assert abs(tot_len_zero - 0.0) < 1e-3
    assert len(pts_zero) == 1
    assert abs(pts_zero[0][0] - 10.0) < 1e-3

    print("test_slider_truncation_and_extension: OK")


def test_ar_defaults_to_od():
    # If ApproachRate is missing in [Difficulty], it must default to OverallDifficulty
    content = """osu file format v14
[General]
Mode: 0
[Difficulty]
CircleSize: 4
OverallDifficulty: 7.25
HPDrainRate: 5
[HitObjects]
256,192,1000,1,0
"""
    with tempfile.NamedTemporaryFile("w", suffix=".osu", delete=False, encoding="utf-8") as f:
        f.write(content)
        tmp_path = f.name

    try:
        bm = parse_osu_beatmap(tmp_path)
        assert abs(bm.od - 7.25) < 1e-3
        assert abs(bm.ar - 7.25) < 1e-3, f"Expected AR to default to OD 7.25, got {bm.ar}"
    finally:
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
    print("test_ar_defaults_to_od: OK")


def test_circle_only_map_without_slider_multiplier():
    # Circle-only map has optional SliderMultiplier in Rust core; verification must pass
    content = """osu file format v14
[General]
Mode: 0
[Difficulty]
CircleSize: 4
OverallDifficulty: 6
ApproachRate: 6
HPDrainRate: 5
[HitObjects]
256,192,1000,1,0
256,192,2000,1,0
"""
    with tempfile.NamedTemporaryFile("w", suffix=".osu", delete=False, encoding="utf-8") as f:
        f.write(content)
        tmp_path = f.name

    try:
        res = verify_beatmap(tmp_path)
        assert res.status == "PASS", f"Verification should pass on circle-only map without SM: {res.reason}"
        assert res.rust_beatmap is not None
        assert len(res.rust_beatmap.hit_objects) == 2
    finally:
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
    print("test_circle_only_map_without_slider_multiplier: OK")


def test_beatmap_from_rust_json():
    # Reconstructing Beatmap from Rust JSON parser output
    micro_map = "tests/data/01_micro/2dearbrave.osu"
    rust_data = run_rust_parser(micro_map)
    assert rust_data.get("status") == "ok"
    bm = beatmap_from_rust_json(rust_data, micro_map)
    assert bm.version == 14
    assert len(bm.hit_objects) == len(rust_data["hit_objects"])
    assert len(bm.timing_points) == len(rust_data["timing_points"])
    assert any(o.kind == "slider" for o in bm.hit_objects)
    print("test_beatmap_from_rust_json: OK")


def test_rust_verification_suite():
    report = run_full_corpus_verification("tests/data")
    assert report["total"] >= 31, f"Expected >= 31 maps, got {report['total']}"
    assert len(report["passed"]) >= 6, f"Expected >= 6 passed, got {len(report['passed'])}"
    assert len(report["failed"]) == 0, f"Expected 0 failed, got {len(report['failed'])}"
    print(f"test_rust_verification_suite: OK ({len(report['passed'])} passed, {len(report['skipped'])} skipped, 0 failed)")


def test_visualizer_headless_playback_and_source_toggle():
    vis = Visualizer(
        "tests/data/01_micro/2dearbrave.osu",
        source="auto",
        width=800,
        height=600,
        headless=True,
    )
    # 2dearbrave is supported by Rust, so auto chooses rust
    assert vis.active_source == "rust"

    # Toggle to Python
    vis.toggle_engine_source()
    assert vis.active_source == "python"

    # Toggle back to Rust
    vis.toggle_engine_source()
    assert vis.active_source == "rust"

    # Test seeking
    vis.seek_to(5000.0)
    assert vis.current_time_ms == 5000.0

    # Step simulation
    vis.step(50.0)
    assert vis.current_time_ms == 5050.0

    # Test pause
    vis.is_playing = False
    vis.step(50.0)
    assert vis.current_time_ms == 5050.0

    # Render a frame
    vis.render()

    # Replay loaded automatically from 2dearbrave.osr
    assert vis.replay is not None
    pos, keys = vis.replay.cursor_at(vis.current_time_ms)
    assert pos is not None
    assert len(pos) == 2

    print("test_visualizer_headless_playback_and_source_toggle: OK")


def test_tech_hud_docking_and_no_overlap():
    map_path = "tests/data/01_micro/2dearbrave.osu"
    vis = Visualizer(map_path, headless=True)

    # 1. Closed HUD: full playfield
    orig_pf = vis.pf_rect.copy()
    assert not vis.show_tech_hud

    # 2. Toggle HUD on (default left dock)
    vis.toggle_tech_hud()
    assert vis.show_tech_hud
    assert vis.tech_hud_dock == "left"
    # Zero overlap check
    assert not vis.pf_rect.colliderect(vis.tech_panel_rect)
    assert vis.pf_rect.x > vis.tech_panel_rect.right

    # Render frame with inspector active
    vis.render()

    # 3. Toggle dock to right
    vis.toggle_tech_hud_dock()
    assert vis.tech_hud_dock == "right"
    # Zero overlap check
    assert not vis.pf_rect.colliderect(vis.tech_panel_rect)
    assert vis.pf_rect.right < vis.tech_panel_rect.x

    vis.render()

    # 4. Toggle HUD off: returns to full playfield
    vis.toggle_tech_hud()
    assert not vis.show_tech_hud
    assert vis.pf_rect == orig_pf

    print("test_tech_hud_docking_and_no_overlap: OK")


if __name__ == "__main__":
    test_linear_curve()
    test_bezier_multi_segment()
    test_circular_arc_normal_and_collinear()
    test_catmull_curve()
    test_slider_truncation_and_extension()
    test_ar_defaults_to_od()
    test_circle_only_map_without_slider_multiplier()
    test_beatmap_from_rust_json()
    test_rust_verification_suite()
    test_visualizer_headless_playback_and_source_toggle()
    test_tech_hud_docking_and_no_overlap()
    print("\nALL AUTOMATED TESTS PASSED!")
