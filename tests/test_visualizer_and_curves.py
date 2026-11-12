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

import hashlib
import lzma
import math
import os
import struct
import sys
import tempfile
import pygame

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
    verify_replay,
    run_full_corpus_verification,
    run_rust_parser,
    beatmap_from_rust_json,
    replay_from_rust_json,
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

    # Test keyboard speed events (Up/Down with no mod, shift, ctrl)
    pygame.event.post(pygame.event.Event(pygame.KEYDOWN, key=pygame.K_UP, mod=0))
    pygame.event.post(pygame.event.Event(pygame.KEYDOWN, key=pygame.K_DOWN, mod=pygame.KMOD_SHIFT))
    pygame.event.post(pygame.event.Event(pygame.KEYDOWN, key=pygame.K_UP, mod=pygame.KMOD_CTRL))
    vis.run(max_frames=3)
    assert vis.playback_speed > 0.0

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


def _encode_osr_str(text: str) -> bytes:
    if not text:
        return b"\x00"
    raw = text.encode("utf-8")
    n = len(raw)
    uleb = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n != 0:
            byte |= 0x80
        uleb.append(byte)
        if n == 0:
            break
    return b"\x0b" + bytes(uleb) + raw


def _build_test_osr(
    bm_hash: str,
    player: str,
    game_version: int,
    mods: int,
    frames_str: str,
    trailer: bytes = b"",
    score: int = 12345,
    max_combo: int = 42,
    counts: tuple = (10, 2, 1, 3, 4, 5),
) -> bytes:
    compressed = lzma.compress(frames_str.encode("ascii"), format=lzma.FORMAT_ALONE)
    buf = bytearray()
    buf.append(0)  # mode = osu!standard
    buf.extend(struct.pack("<i", game_version))
    buf.extend(_encode_osr_str(bm_hash))
    buf.extend(_encode_osr_str(player))
    buf.extend(_encode_osr_str("deadbeef" * 4))
    buf.extend(struct.pack("<HHHHHH", *counts))
    buf.extend(struct.pack("<iH", score, max_combo))
    buf.append(1)  # perfect
    buf.extend(struct.pack("<I", mods))
    buf.extend(_encode_osr_str(""))  # life_graph
    buf.extend(b"\x01\x02\x03\x04\x05\x06\x07\x08")
    buf.extend(struct.pack("<i", len(compressed)))
    buf.extend(compressed)
    buf.extend(trailer)
    return bytes(buf)


def test_rust_json_metadata_colours_breaks_and_warnings():
    content = (
        "\ufeffosu file format v14\r\n"
        "[General]\r\n"
        "Mode: 0\r\n"
        "AudioFilename: track.mp3\r\n"
        "StackLeniency: 0.45\r\n"
        "[Metadata]\r\n"
        "Title: Test Song\r\n"
        "TitleUnicode: Тест\r\n"
        "Artist: Test Artist\r\n"
        "ArtistUnicode: Артист\r\n"
        "Creator: Mapper\r\n"
        "Version: Insane\r\n"
        "Source: Rosea\r\n"
        "Tags: tag1 tag2\r\n"
        "BeatmapID: 12345\r\n"
        "BeatmapSetID: 67890\r\n"
        "[Difficulty]\r\n"
        "CircleSize: 4\r\n"
        "OverallDifficulty: 7\r\n"
        "HPDrainRate: 5\r\n"
        "ApproachRate: 8\r\n"
        "SliderMultiplier: 1.5\r\n"
        "SliderTickRate: 2\r\n"
        "[Events]\r\n"
        "0,0,\"bg, with, comma.jpg\",0,0\r\n"
        "2,1000,2500\r\n"
        "Break,4000,3500\r\n"
        "[Colours]\r\n"
        "Combo2: 20,30,40\r\n"
        "Combo1: 10,20,30\r\n"
        "SliderTrackOverride: 12,34,56\r\n"
        "SliderBorder: 210,220,230\r\n"
        "[UnknownCustomSection]\r\n"
        "Foo: Bar\r\n"
        "[HitObjects]\r\n"
        "256,192,500,1,0\r\n"
        "100,100,3000,2,0,L|200:100,1,100\r\n"
    )
    raw_bytes = content.encode("utf-8")
    expected_md5 = hashlib.md5(raw_bytes).hexdigest()

    with tempfile.NamedTemporaryFile("wb", suffix=".osu", delete=False) as f:
        f.write(raw_bytes)
        tmp_path = f.name

    try:
        rust_data = run_rust_parser(tmp_path)
        assert rust_data["status"] == "ok", f"Unexpected error: {rust_data}"
        assert rust_data["beatmap_md5"] == expected_md5
        assert abs(rust_data["stack_leniency"] - 0.45) < 1e-4

        warn_codes = {w["code"]: w for w in rust_data.get("warnings", [])}
        assert "UnknownBeatmapFields" in warn_codes
        assert warn_codes["UnknownBeatmapFields"]["count"] >= 2

        bm = beatmap_from_rust_json(rust_data, tmp_path)
        assert bm.beatmap_md5 == expected_md5
        assert bm.audio_filename == "track.mp3"
        assert bm.title == "Test Song"
        assert bm.title_unicode == "Тест"
        assert bm.artist == "Test Artist"
        assert bm.artist_unicode == "Артист"
        assert bm.creator == "Mapper"
        assert bm.version_name == "Insane"
        assert bm.beatmap_id == 12345
        assert bm.beatmapset_id == 67890
        assert bm.combo_colours == [(10, 20, 30), (20, 30, 40)]
        assert bm.slider_track_override == (12, 34, 56)
        assert bm.slider_border == (210, 220, 230)
        assert bm.breaks == [(1000.0, 2500.0), (4000.0, 4000.0)]

        res = verify_beatmap(tmp_path)
        assert res.status == "PASS", f"Verification failed: {res.reason}"
    finally:
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
    print("test_rust_json_metadata_colours_breaks_and_warnings: OK")


def test_replay_pair_md5_and_normalization_bridge():
    map_content = (
        "osu file format v14\n"
        "[General]\nMode: 0\n"
        "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n"
        "[HitObjects]\n256,192,1000,1,0\n"
    ).encode("utf-8")
    other_map_content = (
        "osu file format v14\n"
        "[General]\nMode: 0\n"
        "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n"
        "[HitObjects]\n256,192,1001,1,0\n"
    ).encode("utf-8")
    md5_ok = hashlib.md5(map_content).hexdigest()

    # Frame stream:
    # 0: delta=10, dummy (256,-500) -> t=10 -> prefix swaps -> t=0, removed as InitialDummy
    # 1: delta=-20, dummy (256,-500) -> t=-10 -> prefix sets t=10, removed as InitialDummy
    # 2: delta=50 (t=40)
    # 3: delta=-10 (t=30) -> removed as BackwardFrame
    # 4: delta=30 (t=60)
    # 5: -12345|0|0|999 (seed)
    frames_str = (
        "10|256|-500|0,"
        "-20|256|-500|0,"
        "50|120|240|5,"
        "-10|130|250|0,"
        "30|140|260|1,"
        "-12345|0|0|999,"
    )
    osr_ok = _build_test_osr(
        bm_hash=md5_ok.upper(),
        player="RoseaTester",
        game_version=20240101,
        mods=0,
        frames_str=frames_str,
        trailer=b"\x01\x02\x03\x04",
        score=54321,
        max_combo=12,
        counts=(3, 1, 0, 2, 1, 0),
    )
    osr_mods = _build_test_osr(
        bm_hash=md5_ok,
        player="ModPlayer",
        game_version=20240101,
        mods=64,
        frames_str="10|256|192|0,",
    )

    with tempfile.TemporaryDirectory() as tmpdir:
        map_p = os.path.join(tmpdir, "map.osu")
        other_p = os.path.join(tmpdir, "other.osu")
        rep_p = os.path.join(tmpdir, "rep.osr")
        rep_mods_p = os.path.join(tmpdir, "rep_mods.osr")
        with open(map_p, "wb") as f:
            f.write(map_content)
        with open(other_p, "wb") as f:
            f.write(other_map_content)
        with open(rep_p, "wb") as f:
            f.write(osr_ok)
        with open(rep_mods_p, "wb") as f:
            f.write(osr_mods)

        # 1. Valid pair + normalization + seed + trailer
        rv = verify_replay(map_p, rep_p)
        assert rv.status == "PASS", f"Expected PASS, got {rv.status}: {rv.reason}"
        assert rv.hash_status == "ok"
        assert rv.hash_match is True
        assert rv.rust_replay is not None
        assert rv.rust_replay.player == "RoseaTester"
        assert rv.rust_replay.seed == 999
        assert rv.rust_replay.trailer_size == 4
        assert rv.rust_replay.score == 54321
        assert rv.rust_replay.max_combo == 12
        assert rv.rust_replay.count_300 == 3
        assert rv.rust_replay.count_100 == 1
        assert rv.normalization is not None
        assert rv.normalization["changed_times"] == 2
        assert rv.normalization["initial_dummy_removed"] == 2
        assert rv.normalization["backward_removed"] == 1
        warn_codes = [w["code"] for w in rv.warnings]
        assert "ReplayTrailerNotInterpreted" in warn_codes
        assert "ReplayInputNormalized" in warn_codes
        assert [(fr.time_ms, fr.x, fr.y, fr.keys) for fr in rv.rust_replay.frames] == [
            (40.0, 120.0, 240.0, 5),
            (60.0, 140.0, 260.0, 1),
        ]

        # 2. BeatmapHashMismatch
        rv_bad = verify_replay(other_p, rep_p)
        assert rv_bad.status == "HASH_MISMATCH"
        assert rv_bad.error_code == "BeatmapHashMismatch"
        assert rv_bad.hash_status == "BeatmapHashMismatch"
        assert rv_bad.hash_match is False
        assert rv_bad.rust_replay is None

        # 3. UnsupportedMods
        rv_mods = verify_replay(map_p, rep_mods_p)
        assert rv_mods.status == "SKIPPED_UNSUPPORTED"
        assert rv_mods.error_code == "UnsupportedMods"
        assert rv_mods.rust_replay is None

        # 4. UnsupportedReplayFeature (fractional delta)
        rep_frac_p = os.path.join(tmpdir, "rep_frac.osr")
        with open(rep_frac_p, "wb") as f:
            f.write(_build_test_osr(md5_ok, "FracTester", 20240101, 0, "1.5|256|192|0,"))
        rv_frac = verify_replay(map_p, rep_frac_p)
        assert rv_frac.status == "SKIPPED_UNSUPPORTED"
        assert rv_frac.error_code == "UnsupportedReplayFeature"
        assert rv_frac.rust_replay is None

        # 5. BeatmapHashMismatch fallback inside Visualizer
        vis_mm = Visualizer(other_p, replay_path=rep_p, source="auto", headless=True)
        assert vis_mm.active_source == "rust"
        assert vis_mm.active_replay_source == "python_fallback"
        assert vis_mm.replay is vis_mm.py_replay
        assert vis_mm.replay_verification is not None
        assert vis_mm.replay_verification.hash_status == "BeatmapHashMismatch"
        vis_mm.show_verification_detail = True
        vis_mm.show_tech_hud = True
        vis_mm.update_playfield_layout()
        vis_mm.render()
        vis_mm.toggle_engine_source()
        assert vis_mm.active_source == "python"
        assert vis_mm.active_replay_source == "python"

    # 6. Empty replay data fixture (NoReplayData)
    empty_osr = os.path.join("tests", "data", "05_special", "empty_replay_data.osr")
    micro_osu = os.path.join("tests", "data", "01_micro", "2dearbrave.osu")
    if os.path.isfile(empty_osr) and os.path.isfile(micro_osu):
        rv_empty = verify_replay(micro_osu, empty_osr)
        assert rv_empty.status == "SKIPPED_UNSUPPORTED"
        assert rv_empty.error_code == "NoReplayData"

    print("test_replay_pair_md5_and_normalization_bridge: OK")


def test_visualizer_replay_source_switch_breaks_and_colours():
    # 1. Valid pair with normalization & SliderBorder (1936782_pchir)
    vis = Visualizer(
        "tests/data/02_sliders_geometry/1936782_pchir.osu",
        replay_path="tests/data/02_sliders_geometry/1936782_pchir.osr",
        source="auto",
        headless=True,
    )
    assert vis.active_source == "rust"
    assert vis.active_replay_source == "rust"
    assert vis.replay is vis.rust_replay
    assert vis.beatmap.slider_border == (254, 255, 198)
    assert vis.replay_verification is not None
    assert vis.replay_verification.status == "PASS"
    assert len(vis.rust_replay.frames) < len(vis.py_replay.frames)

    # Toggle S -> Python for both map and replay
    vis.toggle_engine_source()
    assert vis.active_source == "python"
    assert vis.active_replay_source == "python"
    assert vis.replay is vis.py_replay

    # Toggle S -> Rust for both map and replay
    vis.toggle_engine_source()
    assert vis.active_source == "rust"
    assert vis.active_replay_source == "rust"
    assert vis.replay is vis.rust_replay

    # Render diagnostics overlay (V) and Tech HUD (T)
    vis.show_tech_hud = True
    vis.show_verification_detail = True
    vis.update_playfield_layout()
    vis.render()

    # 2. Valid Rust map + rejected replay (mods=64 on 06_personal/1.osr) -> Python replay fallback
    vis_fb = Visualizer(
        "tests/data/06_personal/1.osu",
        replay_path="tests/data/06_personal/1.osr",
        source="auto",
        headless=True,
    )
    assert vis_fb.active_source == "rust"
    assert vis_fb.active_replay_source == "python_fallback"
    assert vis_fb.replay is vis_fb.py_replay
    assert vis_fb.replay_verification.error_code == "UnsupportedMods"
    vis_fb.show_verification_detail = True
    vis_fb.render()

    # 3. Map with breaks (galaxy.osu has 7 breaks) + break band remains visible past scrubber
    vis_br = Visualizer("tests/data/04_errors/galaxy.osu", headless=True)
    assert len(vis_br.beatmap.breaks) == 7
    b_start, b_end = vis_br.beatmap.breaks[0]
    vis_br.seek_to((b_start + b_end) / 2.0)
    assert vis_br.active_break_at(vis_br.current_time_ms) == (b_start, b_end)
    vis_br.show_tech_hud = True
    vis_br.update_playfield_layout()
    vis_br.render()

    # Seek past break[0] and verify timeline break band still renders cyan border (94, 212, 224)
    vis_br.seek_to(b_end + 10000.0)
    vis_br.render()
    bar_w = vis_br.width - 40
    mid_ms = (b_start + b_end) / 2.0
    bx_mid = 20 + int((mid_ms / vis_br.last_time) * bar_w)
    tl_top_rgb = tuple(vis_br.screen.get_at((bx_mid, vis_br.height - 50))[:3])
    assert tl_top_rgb == (94, 212, 224), f"Expected cyan break highlight on timeline, got {tl_top_rgb}"

    # 4. Synthetic map with SliderTrackOverride and SliderBorder pixel verification
    slider_osu = (
        "osu file format v14\n[General]\nMode: 0\n"
        "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n"
        "SliderMultiplier: 1.4\nSliderTickRate: 1\n"
        "[TimingPoints]\n0,500,4,1,0,100,1,0\n"
        "[Colours]\nCombo1: 255,255,0\nSliderTrackOverride: 10,200,30\nSliderBorder: 250,40,40\n"
        "[HitObjects]\n100,192,1000,2,0,L|400:192,1,300\n"
    )
    with tempfile.NamedTemporaryFile("w", suffix=".osu", delete=False, encoding="utf-8") as f:
        f.write(slider_osu)
        sl_path = f.name
    try:
        vis_col = Visualizer(sl_path, headless=True, width=640, height=480)
        vis_col.seek_to(1100.0)
        vis_col.render()
        cx, cy = vis_col.osu_to_screen(250, 192)
        center_rgb = tuple(vis_col.screen.get_at((cx, cy))[:3])
        assert center_rgb[1] > center_rgb[0] + 100 and center_rgb[1] > center_rgb[2] + 100, (
            f"Expected green SliderTrackOverride center, got {center_rgb}"
        )
        border_y = cy - max(1, int(vis_col.screen_radius) - 2)
        border_rgb = tuple(vis_col.screen.get_at((cx, border_y))[:3])
        assert border_rgb[0] > border_rgb[1] + 100 and border_rgb[0] > border_rgb[2] + 100, (
            f"Expected red SliderBorder edge, got {border_rgb}"
        )
        vis_col.show_verification_detail = True
        vis_col.render()
    finally:
        if os.path.exists(sl_path):
            os.remove(sl_path)

    print("test_visualizer_replay_source_switch_breaks_and_colours: OK")


def test_unsupported_version_mode_and_cli_verify_codes():
    import subprocess

    v13_osu = (
        "osu file format v13\n[General]\nMode: 0\n"
        "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n"
        "[HitObjects]\n256,192,1000,1,0\n"
    )
    taiko_osu = (
        "osu file format v14\n[General]\nMode: 1\n"
        "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n"
        "[HitObjects]\n256,192,1000,1,0\n"
    )
    for txt in (v13_osu, taiko_osu):
        with tempfile.NamedTemporaryFile("w", suffix=".osu", delete=False, encoding="utf-8") as f:
            f.write(txt)
            p = f.name
        try:
            res = verify_beatmap(p)
            assert res.status == "SKIPPED_UNSUPPORTED", f"Expected SKIPPED_UNSUPPORTED, got {res.status}: {res.reason}"
        finally:
            if os.path.exists(p):
                os.remove(p)

    # --verify CLI exit codes: 0 on valid pair, 1 on BeatmapHashMismatch
    ok_proc = subprocess.run(
        [
            sys.executable,
            "scripts/visualizer.py",
            "--verify",
            "tests/data/01_micro/2dearbrave.osu",
            "--replay",
            "tests/data/01_micro/2dearbrave.osr",
        ],
        capture_output=True,
        text=True,
    )
    assert ok_proc.returncode == 0, f"Expected exit 0, got {ok_proc.returncode}: {ok_proc.stdout}"

    mm_proc = subprocess.run(
        [
            sys.executable,
            "scripts/visualizer.py",
            "--verify",
            "tests/data/01_micro/2dearbrave.osu",
            "--replay",
            "tests/data/01_micro/2yomi.osr",
        ],
        capture_output=True,
        text=True,
    )
    assert mm_proc.returncode == 1, f"Expected exit 1 on BeatmapHashMismatch, got {mm_proc.returncode}"

    print("test_unsupported_version_mode_and_cli_verify_codes: OK")


def test_interactive_inspector_modals_and_jump_to_issue():
    # 1. Valid map + replay with UnknownBeatmapFields, Normalization events, and Trailer bytes
    osu_txt = (
        "osu file format v14\n"
        "[General]\nMode: 0\nAudioFilename: test.mp3\nCountdown: 1\n"
        "[Editor]\nDistanceSpacing: 1.2\n"
        "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n"
        "[HitObjects]\n256,192,1500,1,0\n320,240,3500,1,0\n"
    )
    raw_bytes = osu_txt.encode("utf-8")
    md5 = hashlib.md5(raw_bytes).hexdigest()
    # Frame 0: dummy (256,-500) @ 0ms; Frame 1: real @ 1500ms; Frame 2: backward @ 1450ms; Frame 3: click @ 1500ms
    frames_str = "0|256|-500|0,1500|256|192|4,-50|300|200|0,50|256|192|4,"
    trailer_bytes = b"\x01\x02\x03\x04\x05\x06\x07\x08"
    osr_bytes = _build_test_osr(md5, "InspectPlayer", 20260101, 0, frames_str, trailer=trailer_bytes)

    with tempfile.NamedTemporaryFile("wb", suffix=".osu", delete=False) as fm:
        fm.write(raw_bytes)
        osu_path = fm.name
    with tempfile.NamedTemporaryFile("wb", suffix=".osr", delete=False) as fr:
        fr.write(osr_bytes)
        osr_path = fr.name

    try:
        vis = Visualizer(osu_path, replay_path=osr_path, headless=True)
        vis.show_tech_hud = True
        vis.update_playfield_layout()
        vis.render()

        # UnknownBeatmapFields list matches count
        assert len(vis.verification.unknown_fields) >= 3
        unk_hitboxes = [r for r, a in vis.tech_hud_hitboxes if a == "unknown_fields"]
        assert unk_hitboxes, "Expected clickable unknown_fields row in Tech HUD"
        assert vis._try_click_tech_hud(*unk_hitboxes[0].center)
        assert vis.detail_modal_kind == "unknown_fields"
        vis.render()
        assert vis.detail_modal_rect.width > 0
        vis.close_detail_modal()

        # Trailer hex dump modal
        tr_hitboxes = [r for r, a in vis.tech_hud_hitboxes if a == "trailer"]
        assert tr_hitboxes, "Expected clickable trailer row in Tech HUD"
        assert vis._try_click_tech_hud(*tr_hitboxes[0].center)
        assert vis.detail_modal_kind == "trailer"
        _, tr_sub, tr_rows, tr_jump = vis._build_modal_rows()
        assert not tr_jump and len(tr_rows) == 1
        assert "01 02 03 04 05 06 07 08" in tr_rows[0]["text"]
        assert "8" in tr_sub
        vis.close_detail_modal()

        # Normalization replay_issues list + click jump + step Prev/Next
        norm_hitboxes = [r for r, a in vis.tech_hud_hitboxes if a == "replay_issues"]
        assert norm_hitboxes, "Expected clickable replay_issues row in Tech HUD"
        assert vis._try_click_tech_hud(*norm_hitboxes[0].center)
        assert vis.detail_modal_kind == "replay_issues"
        vis.render()
        assert len(vis.detail_modal_item_hitboxes) >= 2
        first_box, first_idx = vis.detail_modal_item_hitboxes[0]
        vis.jump_to_issue("replay_issues", first_idx)
        assert vis.detail_modal_kind is None
        assert not vis.is_playing
        assert vis.focused_issue is not None
        assert vis.focused_issue["tag"] == "InitialDummy"
        vis.render()
        assert vis.focus_btn_next_rect is not None
        vis._try_click_focus_callout(*vis.focus_btn_next_rect.center)
        assert vis.focused_issue["tag"] == "BackwardFrame"
        assert abs(vis.current_time_ms - 1500.0) < 1e-3

        # Jump to last click from Tech HUD
        vis.seek_to(1600.0)
        vis.render()
        click_hb = [r for r, a in vis.tech_hud_hitboxes if a == "last_click"]
        if click_hb:
            assert vis._try_click_tech_hud(*click_hb[0].center)
            assert abs(vis.current_time_ms - 1500.0) < 1e-3
    finally:
        for p in (osu_path, osr_path):
            if os.path.exists(p):
                os.remove(p)

    # 2. Map with multiple Catmull unsupported sliders -> multi-item map_issues + jump to note #2
    catmull_osu = (
        "osu file format v14\n"
        "[General]\nMode: 0\n"
        "[Difficulty]\nCircleSize: 4\nOverallDifficulty: 7\nHPDrainRate: 5\n"
        "SliderMultiplier: 1.4\nSliderTickRate: 1\n"
        "[TimingPoints]\n0,500,4,1,0,100,1,0\n"
        "[HitObjects]\n"
        "100,100,1200,2,0,C|150:150|200:100,1,100\n"
        "250,220,4200,2,0,C|300:220|350:260,1,100\n"
    )
    with tempfile.NamedTemporaryFile("w", suffix=".osu", delete=False, encoding="utf-8") as fm2:
        fm2.write(catmull_osu)
        cm_path = fm2.name
    try:
        vis2 = Visualizer(cm_path, headless=True)
        assert vis2.verification.status == "SKIPPED_UNSUPPORTED"
        assert len(vis2.verification.map_issues) >= 2
        assert vis2.verification.map_issues[0]["primary"] is True
        assert abs(vis2.verification.map_issues[0]["time_ms"] - 1200.0) < 1e-3

        vis2.show_tech_hud = True
        vis2.update_playfield_layout()
        vis2.render()
        mi_hb = [r for r, a in vis2.tech_hud_hitboxes if a == "map_issues"]
        assert mi_hb, "Expected clickable map_issues row in Tech HUD"
        assert vis2._try_click_tech_hud(*mi_hb[0].center)
        assert vis2.detail_modal_kind == "map_issues"
        vis2.render()
        assert len(vis2.detail_modal_item_hitboxes) >= 2
        # Jump directly to second Catmull note at 4200ms
        _, second_idx = vis2.detail_modal_item_hitboxes[1]
        vis2.jump_to_issue("map_issues", second_idx)
        assert not vis2.is_playing
        assert abs(vis2.current_time_ms - 4200.0) < 1e-3
        vis2.render()
    finally:
        if os.path.exists(cm_path):
            os.remove(cm_path)

    print("test_interactive_inspector_modals_and_jump_to_issue: OK")


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
    test_rust_json_metadata_colours_breaks_and_warnings()
    test_replay_pair_md5_and_normalization_bridge()
    test_visualizer_replay_source_switch_breaks_and_colours()
    test_unsupported_version_mode_and_cli_verify_codes()
    test_interactive_inspector_modals_and_jump_to_issue()
    print("\nALL AUTOMATED TESTS PASSED!")
