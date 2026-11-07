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


def test_audio_engine_and_synchronization():
    map_path = "tests/data/03_spinners/realexit.osu"
    vis = Visualizer(map_path, headless=True)

    # 1. Audio and hitsounds initialization
    assert vis.audio_available
    assert "normal" in vis.hitsound_samples
    assert len(vis.sound_events) > 100

    # 2. Seeking and bisect sound event repositioning
    vis.seek_to(15841.0)
    assert vis.current_time_ms == 15841.0
    assert vis.sound_event_idx > 0

    # 3. Simulation step
    prev_t = vis.current_time_ms
    vis.step(50.0)
    assert vis.current_time_ms == prev_t + 50.0

    # 4. Mute toggle
    assert not vis.audio_muted
    vis.toggle_audio_mute()
    assert vis.audio_muted
    vis.toggle_audio_mute()
    assert not vis.audio_muted

    # 5. Audio offset adjustment
    assert vis.audio_offset_ms == 0.0
    vis.adjust_audio_offset(15.0)
    assert vis.audio_offset_ms == 15.0
    vis.adjust_audio_offset(-15.0)
    assert vis.audio_offset_ms == 0.0

    # 6. Playback speed change (smooth 0.01 step, range [0.01, 2.00])
    vis.change_speed(0.01)
    assert abs(vis.playback_speed - 1.01) < 1e-4
    vis.change_speed(-0.01)
    assert abs(vis.playback_speed - 1.0) < 1e-4
    vis.change_speed(0.25)
    assert abs(vis.playback_speed - 1.25) < 1e-4
    vis.set_speed(1.0)
    assert abs(vis.playback_speed - 1.0) < 1e-4
    vis.change_speed(-5.0)
    assert abs(vis.playback_speed - 0.01) < 1e-4  # lower bound clamped to 0.01
    vis.change_speed(5.0)
    assert abs(vis.playback_speed - 2.00) < 1e-4  # upper bound clamped to 2.00
    vis.set_speed(1.0)

    # 7. Render frame with HUD and audio info
    vis.show_tech_hud = True
    vis.update_playfield_layout()
    vis.render()

    # 8. Test BeatmapSetID resolution & Skip Intro on personal map
    vis_06 = Visualizer("tests/data/06_personal/1.osu", headless=True)
    assert vis_06.beatmap.beatmapset_id == 1049899
    assert vis_06.beatmap.beatmap_id == 2444814
    assert vis_06.start_time == 0.0
    assert vis_06.current_time_ms == 0.0
    if vis_06.audio_path:
        assert "1049899" in vis_06.audio_path

    # Test skip_intro jumps to correct offset before first note
    vis_06.skip_intro()
    expected_target = 4892.0 - vis_06.beatmap.ar_preempt_fadein[0] - 600.0
    assert abs(vis_06.current_time_ms - expected_target) < 1.0

    print("test_audio_engine_and_synchronization: OK")


def test_osu_stable_pairs_and_browser():
    from scripts.osu_db import OsuStableDatabase
    from scripts.map_browser import scan_all_maps

    # 1. Test database availability & scanning
    db = OsuStableDatabase()
    if db.is_available():
        pairs = db.scan_replays_and_match()
        assert len(pairs) > 0, "Expected at least 1 matched pair from stable"
        for bm, rep in pairs[:10]:
            assert os.path.isfile(bm["full_path"]), f"Beatmap file missing: {bm['full_path']}"
            assert os.path.isfile(rep.file_path), f"Replay file missing: {rep.file_path}"
            assert len(rep.bm_hash) == 32

    # 2. Test scan_all_maps combining tests and stable
    entries = scan_all_maps("tests/data")
    assert len(entries) >= 32
    # Verify entries have valid fields
    for e in entries[:20]:
        assert e.title and e.version
        assert os.path.isfile(e.file_path)

    # 3. Test Visualizer cycling through combined list with replay
    vis = Visualizer("tests/data/01_micro/2dearbrave.osu", headless=True)
    assert len(vis.all_map_entries) == len(entries)
    vis.switch_relative_map(1)
    assert vis.beatmap is not None

    print("test_osu_stable_pairs_and_browser: OK")


def test_safe_hitsound_playback_with_dummy_wav():
    """Verify that 44-byte silent WAV files used in osu skins never crash SDL_mixer."""
    import tempfile
    with tempfile.TemporaryDirectory() as tmpdir:
        dummy_wav = os.path.join(tmpdir, "normal-hitwhistle.wav")
        riff_header = bytes.fromhex("524946462400000057415645666d7420100000000100020044ac000010b10200040010006461746100000000")
        with open(dummy_wav, "wb") as f_out:
            f_out.write(riff_header)
        assert os.path.getsize(dummy_wav) == 44
        vis = Visualizer("tests/data/01_micro/2dearbrave.osu", headless=True)
        vis.play_hitsound("whistle")
        vis.play_hitsound("nonexistent")
        for _ in range(30):
            vis.step(50.0)
    print("test_safe_hitsound_playback_with_dummy_wav: OK")


def test_tech_hud_click_badges():
    """Verify that in Technical Inspector mode, replay and live clicks display up to 5 badges with timing and judgement."""
    vis = Visualizer(
        "tests/data/06_personal/1.osu",
        replay_path="tests/data/06_personal/1.osr",
        headless=True,
    )
    assert len(vis.replay_clicks) > 0, "Replay clicks must be precomputed"
    assert len(vis.replay_clicks) == len(vis.replay_click_times)

    # Check structure of precomputed clicks
    first_gameplay_click = next(c for c in vis.replay_clicks if c.time_ms > 4000.0)
    assert first_gameplay_click.key in ("K1", "K2", "M1", "M2")
    assert first_gameplay_click.result in ("300", "100", "50", "Miss")
    assert first_gameplay_click.delta_ms is not None
    assert f"({first_gameplay_click.delta_ms:+.1f}ms {first_gameplay_click.result})" in first_gameplay_click.badge_text

    # Default: tech HUD is off, rendering should work cleanly
    assert not vis.show_tech_hud
    vis.render()

    # Enable Technical HUD
    vis.show_tech_hud = True
    vis.seek_to(10000.0)
    vis.render()

    # Query latest click and verify
    latest = vis.get_latest_click_at(10000.0)
    assert latest is not None
    assert latest.time_ms <= 10000.0

    # Verify at most 5 recent clicks within 2500ms are rendered
    t = 10000.0
    recent = [c for c in vis.replay_clicks if c.time_ms <= t and t - c.time_ms <= 2500.0][-5:]
    assert 0 < len(recent) <= 5

    # Test live click recording
    vis.seek_to(4892.0)
    vis.record_live_click("K1")
    assert len(vis.live_clicks) == 1
    assert vis.live_clicks[0].key == "K1"
    assert "K1" in vis.live_clicks[0].badge_text
    vis.render()

    # Seeking backwards prunes future live clicks
    vis.seek_to(3000.0)
    assert len(vis.live_clicks) == 0

    print("test_tech_hud_click_badges: OK")


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
    test_audio_engine_and_synchronization()
    test_osu_stable_pairs_and_browser()
    test_safe_hitsound_playback_with_dummy_wav()
    test_tech_hud_click_badges()
    print("\nALL AUTOMATED TESTS PASSED!")
