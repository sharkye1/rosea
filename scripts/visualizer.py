"""
Rosea osu! Beatmap Visualizer and Parser Tester
Supports:
- All hit objects: Hit Circles, Sliders (L, B, P, C), Spinners
- 512x384 osu! playfield with proper aspect ratio scaling and letterboxing
- Full timing, SV multipliers, slider velocity, slider ticks, and repeats
- Approach circles (AR preempt / fade-in), Circle Size (CS) scaling
- Stacking calculation
- Playback controls: Play/Pause (Space), Seek (Left/Right), Speed (Up/Down), Restart (R/Home)
- Interactive timeline seekbar with object distribution ticks
- Real-time Rust Parser verification badge and diagnostics overlay (Tab/F1, V)
- Headless test mode for automated verification
"""

import math
import bisect
import struct
import re
import os
import sys
import time
import warnings
from typing import List, Tuple, Optional, Dict, Any

# Suppress harmless pkg_resources deprecation warning inside pygame
warnings.filterwarnings("ignore", category=UserWarning, module="pygame.pkgdata")

# Ensure paths
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
for p in (REPO_ROOT, SCRIPT_DIR):
    if p not in sys.path:
        sys.path.insert(0, p)

try:
    from osu_parser import parse_osu_beatmap, Beatmap, HitObject, parse_osr_replay, Replay
    from rust_bridge import (
        verify_beatmap,
        VerificationResult,
        ReplayVerificationResult,
        run_full_corpus_verification,
        beatmap_from_rust_json,
        get_rust_commit_hash,
    )
    from map_browser import MapBrowserManager, scan_all_maps, MapEntry
except ImportError:
    from scripts.osu_parser import (
        parse_osu_beatmap,
        Beatmap,
        HitObject,
        parse_osr_replay,
        Replay,
    )
    from scripts.rust_bridge import (
        verify_beatmap,
        VerificationResult,
        ReplayVerificationResult,
        run_full_corpus_verification,
        beatmap_from_rust_json,
        get_rust_commit_hash,
    )
    from scripts.map_browser import MapBrowserManager, scan_all_maps, MapEntry

import pygame


class ClickEvent:
    """Represents a replay or interactive key press event with timing judgement."""

    def __init__(
        self,
        time_ms: float,
        x: float,
        y: float,
        key: str,
        delta_ms: Optional[float],
        result: str,
        color: Tuple[int, int, int],
        badge_text: str,
    ):
        self.time_ms = time_ms
        self.x = x
        self.y = y
        self.key = key
        self.delta_ms = delta_ms
        self.result = result
        self.color = color
        self.badge_text = badge_text


class Visualizer:
    def __init__(
        self,
        beatmap_path: str,
        replay_path: Optional[str] = None,
        source: str = "auto",
        width: int = 1024,
        height: int = 768,
        headless: bool = False,
        audio_path: Optional[str] = None,
        disable_audio: bool = False,
        disable_hitsounds: bool = False,
        audio_offset: float = 0.0,
        osu_dir: Optional[str] = None,
    ):
        self.beatmap_path = beatmap_path
        self.replay_path = replay_path
        self.requested_source = source
        self.width = width
        self.height = height
        self.headless = headless
        self.osu_dir = os.path.normpath(osu_dir) if osu_dir else os.path.join(os.path.expanduser("~"), "AppData", "Local", "osu!")
        self.toast_message = ""
        self.toast_until = 0.0
        self.rust_commit_hash: str = get_rust_commit_hash()

        if self.headless:
            os.environ["SDL_VIDEODRIVER"] = "dummy"

        pygame.init()
        pygame.display.set_caption(f"Rosea Visualizer - {os.path.basename(beatmap_path)}")

        if self.headless:
            self.screen = pygame.display.set_mode((self.width, self.height))
        else:
            self.screen = pygame.display.set_mode((self.width, self.height), pygame.RESIZABLE)

        self.clock = pygame.time.Clock()
        self.init_fonts()

        # Playback & UI state
        self.playback_speed: float = 1.0
        self.show_hud: bool = True
        self.show_tech_hud: bool = False
        self.show_verification_detail: bool = False
        self.is_scrubbing: bool = False
        self.slider_caches: Dict[int, Tuple[pygame.Surface, int, int]] = {}
        self.btn_maps_rect: pygame.Rect = pygame.Rect(0, 0, 0, 0)
        self.btn_tech_rect: pygame.Rect = pygame.Rect(0, 0, 0, 0)
        self.btn_skip_rect: Optional[pygame.Rect] = None
        self.tech_hud_dock: str = "left"
        self.tech_panel_rect: pygame.Rect = pygame.Rect(0, 0, 0, 0)
        self.btn_tech_dock_rect: pygame.Rect = pygame.Rect(0, 0, 0, 0)
        self.btn_tech_close_rect: pygame.Rect = pygame.Rect(0, 0, 0, 0)

        # Smooth keyboard repeat state for speed control
        self.speed_key_held: Optional[int] = None
        self.speed_key_press_time: float = 0.0
        self.speed_key_last_repeat: float = 0.0

        # Click Badges & Real-time Judgements (Last 5 Clicks on Playfield in Tech HUD)
        self.replay_clicks: List[ClickEvent] = []
        self.replay_click_times: List[float] = []
        self.live_clicks: List[ClickEvent] = []

        # Audio & High-Resolution Clock State
        self.audio_available: bool = False
        self.explicit_audio_path: Optional[str] = audio_path
        self.disable_audio_playback: bool = disable_audio
        self.hitsounds_enabled: bool = not disable_hitsounds
        self.audio_offset_ms: float = audio_offset
        self.audio_muted: bool = False
        self.music_volume: float = 0.75
        self.hitsound_volume: float = 0.8
        self.music_loaded: bool = False
        self.music_playing: bool = False
        self.audio_path: Optional[str] = None
        self.hitsound_samples: Dict[str, pygame.mixer.Sound] = {}
        self.sound_events: List[Tuple[float, str]] = []
        self.sound_event_idx: int = 0
        self.audio_anchor_track_ms: float = 0.0
        self.audio_anchor_perf_time: float = time.perf_counter()

        self.init_audio()

        # Map list for keyboard cycling ([ / ] or PageUp / PageDown)
        self.all_map_entries: List[MapEntry] = []
        self.all_map_files: List[str] = []
        self.current_map_idx: int = 0
        try:
            self.all_map_entries = scan_all_maps(
                os.path.join(REPO_ROOT, "tests", "data"),
                osu_dir=self.osu_dir,
            )
            self.all_map_files = [e.file_path for e in self.all_map_entries]
        except Exception:
            pass

        # Non-blocking Map Browser companion window
        self.map_browser: Optional[MapBrowserManager] = None
        if not self.headless:
            try:
                self.map_browser = MapBrowserManager(
                    root_dir=os.path.join(REPO_ROOT, "tests", "data"),
                    osu_dir=self.osu_dir,
                )
                self.map_browser.start()
            except Exception as e:
                print(f"Warning: could not start MapBrowser: {e}", file=sys.stderr)

        # Initial beatmap load
        self.load_beatmap(beatmap_path, replay_path)

    def _apply_active_source(self):
        """Synchronize active beatmap and replay objects with self.active_source."""
        if self.active_source == "rust" and self.rust_beatmap is not None:
            self.beatmap = self.rust_beatmap
            if self.rust_replay is not None:
                self.replay = self.rust_replay
                self.active_replay_source = "rust"
            elif self.py_replay is not None:
                self.replay = self.py_replay
                self.active_replay_source = "python_fallback"
            else:
                self.replay = None
                self.active_replay_source = "none"
        else:
            self.active_source = "python"
            self.beatmap = self.py_beatmap
            if self.py_replay is not None:
                self.replay = self.py_replay
                self.active_replay_source = "python"
            else:
                self.replay = None
                self.active_replay_source = "none"

        self.live_clicks = []
        self.replay_clicks = []
        self.replay_click_times = []
        if self.replay:
            self.precompute_replay_clicks()

    def load_beatmap(self, beatmap_path: str, replay_path: Optional[str] = None):
        """Dynamically load or switch beatmap on the fly without restarting window."""
        norm_path = os.path.normpath(beatmap_path)
        if not os.path.isfile(norm_path):
            self.set_toast(f"File not found: {os.path.basename(beatmap_path)}", 3.0)
            return

        self.beatmap_path = norm_path
        pygame.display.set_caption(f"Rosea Visualizer - {os.path.basename(norm_path)}")

        # Parse with Python parser
        try:
            self.py_beatmap: Beatmap = parse_osu_beatmap(norm_path)
        except Exception as e:
            self.set_toast(f"Failed to parse map: {e}", 3.0)
            return

        # Resolve replay path before Rust verification so both are checked together
        target_osr = replay_path
        explicit_replay_missing = bool(replay_path and not os.path.isfile(replay_path))
        if explicit_replay_missing:
            print(
                f"Warning: replay file not found: {replay_path}",
                file=sys.stderr,
            )
            target_osr = None
        if not target_osr:
            candidate = os.path.splitext(norm_path)[0] + ".osr"
            if os.path.isfile(candidate):
                target_osr = candidate
        target_osr_valid = target_osr if (target_osr and os.path.isfile(target_osr)) else None
        self.replay_path = target_osr_valid

        # Verification with Rust parser (beatmap + optional replay)
        self.verification: VerificationResult = verify_beatmap(
            norm_path, replay_path=target_osr_valid
        )
        self.rust_beatmap: Optional[Beatmap] = self.verification.rust_beatmap
        self.replay_verification: Optional[ReplayVerificationResult] = (
            self.verification.replay_verification
        )

        # Load Python & Rust Replay objects
        self.py_replay: Optional[Replay] = (
            parse_osr_replay(target_osr_valid) if target_osr_valid else None
        )
        self.rust_replay: Optional[Replay] = (
            self.replay_verification.rust_replay
            if self.replay_verification is not None
            else None
        )
        self.replay: Optional[Replay] = None
        self.active_replay_source: str = "none"
        self.show_replay = True
        self.replay_trail = []

        # Choose active data source
        if self.requested_source == "rust":
            if self.rust_beatmap:
                self.active_source = "rust"
            else:
                self.active_source = "python"
                self.set_toast(
                    f"Rust unavailable ({self.verification.reason[:40]}); using Python",
                    3.5,
                )
        elif self.requested_source == "python":
            self.active_source = "python"
        else:  # "auto"
            if self.rust_beatmap and self.verification.status == "PASS":
                self.active_source = "rust"
            else:
                self.active_source = "python"

        self._apply_active_source()

        # Update index in all_map_entries / all_map_files
        if self.all_map_entries:
            for idx, entry in enumerate(self.all_map_entries):
                if os.path.abspath(entry.file_path) == os.path.abspath(norm_path):
                    self.current_map_idx = idx
                    break
        elif self.all_map_files:
            for idx, p in enumerate(self.all_map_files):
                if os.path.abspath(p) == os.path.abspath(norm_path):
                    self.current_map_idx = idx
                    break

        # Playfield metrics & timing
        self.update_playfield_layout()

        first_time = self.beatmap.hit_objects[0].time if self.beatmap.hit_objects else 0.0
        preempt, _ = self.beatmap.ar_preempt_fadein
        # In osu!, playback starts from the beginning of the audio track (t = 0.0 ms),
        # or earlier if early hit objects require approach circle lead-in before 0.0 ms.
        self.start_time = min(0.0, first_time - preempt - 500.0)
        self.last_time = (
            max(
                [o.end_time for o in self.beatmap.hit_objects]
                + [b[1] for b in getattr(self.beatmap, "breaks", [])]
                + [first_time + 1000.0]
            )
            + 1500.0
        )
        self.current_time_ms = self.start_time
        self.is_playing = True

        self.slider_caches.clear()
        self.build_slider_caches()

        commit_tag = f" [{self.rust_commit_hash}]" if self.rust_commit_hash and self.rust_commit_hash != "unknown" else ""
        engine_name = f"Rust rosea-core{commit_tag}" if self.active_source == "rust" else "Python Standalone"
        replay_name = ""
        if self.replay:
            if self.active_replay_source == "python_fallback" and self.replay_verification:
                err_tag = self.replay_verification.error_code or self.replay_verification.status
                replay_name = f" + Replay ({self.replay.player}, Py fallback: {err_tag})"
            else:
                replay_name = f" + Replay ({self.replay.player})"
        title_disp = self.beatmap.title or "Untitled"
        ver_disp = self.beatmap.version_name or "Normal"
        missing_note = (
            f" | Replay missing: {os.path.basename(replay_path)}"
            if explicit_replay_missing and replay_path
            else ""
        )
        self.set_toast(
            f">> {title_disp} [{ver_disp}] ({engine_name}{replay_name}{missing_note})",
            3.0,
        )

        self.setup_audio_for_beatmap()

    def open_map_browser(self):
        """Open or bring to front the non-blocking map browser window."""
        if self.map_browser:
            self.map_browser.show()
            self.set_toast("Окно выбора карты открыто", 1.5)
        else:
            self.set_toast("Map Browser недоступен в headless режиме", 2.0)

    def switch_relative_map(self, delta: int):
        """Quickly switch to previous/next map in the list (with paired replay)."""
        if self.all_map_entries:
            self.current_map_idx = (self.current_map_idx + delta) % len(self.all_map_entries)
            target_entry = self.all_map_entries[self.current_map_idx]
            self.load_beatmap(target_entry.file_path, replay_path=target_entry.replay_path)
            return

        if not self.all_map_files:
            return
        self.current_map_idx = (self.current_map_idx + delta) % len(self.all_map_files)
        target = self.all_map_files[self.current_map_idx]
        self.load_beatmap(target)

    def set_toast(self, message: str, duration_sec: float = 2.5):
        self.toast_message = message
        self.toast_until = time.time() + duration_sec

    def toggle_engine_source(self):
        """Toggle between Rust and Python beatmap + replay data sources in real time."""
        if self.active_source == "rust":
            self.active_source = "python"
            self._apply_active_source()
            rep_note = " + Replay" if self.replay else ""
            self.set_toast(f"Data source switched to: Python Standalone (Map{rep_note})", 2.5)
            self.update_playfield_layout()
            self.build_slider_caches()
        else:
            if self.rust_beatmap:
                self.active_source = "rust"
                self._apply_active_source()
                commit_tag = f" [{self.rust_commit_hash}]" if self.rust_commit_hash and self.rust_commit_hash != "unknown" else ""
                if self.active_replay_source == "rust":
                    msg = f"Data source switched to: Rust rosea-core{commit_tag} (Map + Replay)"
                elif self.active_replay_source == "python_fallback" and self.replay_verification:
                    err_tag = self.replay_verification.error_code or self.replay_verification.status
                    msg = f"Rust map{commit_tag} + Python replay fallback ({err_tag})"
                else:
                    msg = f"Data source switched to: Rust rosea-core{commit_tag}"
                self.set_toast(msg, 2.5)
                self.update_playfield_layout()
                self.build_slider_caches()
            else:
                self.set_toast(
                    f"Rust unavailable ({self.verification.reason[:45]})", 3.0
                )

    def toggle_tech_hud(self):
        """Toggle technical inspector overlay and re-scale playfield so they do not overlap."""
        self.show_tech_hud = not self.show_tech_hud
        self.update_playfield_layout()
        self.build_slider_caches()
        self.set_toast(
            f"Технический инспектор {'включён' if self.show_tech_hud else 'выключен'}",
            1.5,
        )

    def toggle_tech_hud_dock(self):
        """Toggle dock side of technical inspector between left and right sidebar."""
        self.tech_hud_dock = "right" if self.tech_hud_dock == "left" else "left"
        self.update_playfield_layout()
        self.build_slider_caches()
        side_name = "справа" if self.tech_hud_dock == "right" else "слева"
        self.set_toast(f"Панель инспектора закреплена {side_name}", 1.5)

    def init_audio(self):
        """Initialize audio mixer with low-latency buffer (512 samples) and load hitsounds."""
        try:
            if not pygame.mixer.get_init():
                pygame.mixer.init(frequency=44100, size=-16, channels=2, buffer=512)
                try:
                    pygame.mixer.set_num_channels(32)
                except Exception:
                    pass
            self.audio_available = True
            self.init_hitsounds()
        except Exception:
            self.audio_available = False
            self.hitsounds_enabled = False
            self.disable_audio_playback = True

    def init_hitsounds(self):
        """Load skin hitsounds or synthesize crisp procedural click sounds."""
        if not self.audio_available:
            return

        def _safe_load_sound(path: str) -> Optional[pygame.mixer.Sound]:
            if not os.path.isfile(path):
                return None
            try:
                # In osu! skins, silent dummy files are 44-byte WAV headers with 0 audio payload.
                # Calling pygame.mixer.Sound on 0-sample files causes native crashes in SDL_mixer.
                if os.path.getsize(path) <= 44:
                    return None
                snd = pygame.mixer.Sound(path)
                if getattr(snd, "get_length", lambda: 0.0)() < 0.005:
                    return None
                if len(snd.get_raw()) == 0:
                    return None
                return snd
            except Exception:
                return None

        skins_dir = (
            os.path.join(self.osu_dir, "Skins")
            if self.osu_dir and os.path.isdir(os.path.join(self.osu_dir, "Skins"))
            else os.path.join(os.path.expanduser("~"), "AppData", "Local", "osu!", "Skins")
        )
        if os.path.isdir(skins_dir):
            skin_candidates = []
            try:
                for sname in os.listdir(skins_dir):
                    p = os.path.join(skins_dir, sname)
                    if os.path.isdir(p):
                        if "WhiteCat" in sname:
                            skin_candidates.insert(0, p)
                        else:
                            skin_candidates.append(p)
            except Exception:
                pass

            for skin_path in skin_candidates:
                norm_snd = _safe_load_sound(os.path.join(skin_path, "normal-hitnormal.wav"))
                if norm_snd:
                    self.hitsound_samples["normal"] = norm_snd
                    for kind, fname in [
                        ("whistle", "normal-hitwhistle.wav"),
                        ("finish", "normal-hitfinish.wav"),
                        ("clap", "normal-hitclap.wav"),
                    ]:
                        cand_snd = _safe_load_sound(os.path.join(skin_path, fname))
                        if cand_snd:
                            self.hitsound_samples[kind] = cand_snd
                    break

            needed = [k for k in ["whistle", "finish", "clap"] if k not in self.hitsound_samples]
            if needed:
                for skin_path in skin_candidates:
                    for kind in list(needed):
                        fname = f"normal-hit{kind}.wav"
                        cand_snd = _safe_load_sound(os.path.join(skin_path, fname))
                        if cand_snd:
                            self.hitsound_samples[kind] = cand_snd
                            needed.remove(kind)
                    if not needed:
                        break

        # Procedural fallback for normal hit sound
        if "normal" not in self.hitsound_samples:
            sr = 44100
            dur = 0.04
            buf = bytearray()
            for i in range(int(sr * dur)):
                t = i / float(sr)
                env = math.exp(-t * 90.0)
                sample = int(32767.0 * 0.7 * env * math.sin(2.0 * math.pi * 1100.0 * (1.0 - t * 4.0) * t))
                sample = max(-32767, min(32767, sample))
                buf.extend(struct.pack("<hh", sample, sample))
            try:
                self.hitsound_samples["normal"] = pygame.mixer.Sound(buffer=bytes(buf))
            except Exception:
                pass

        for snd in self.hitsound_samples.values():
            try:
                snd.set_volume(self.hitsound_volume)
            except Exception:
                pass

    def resolve_audio_file(
        self,
        bm_path: str,
        audio_filename: str,
        artist: str,
        title: str,
        beatmapset_id: Optional[int] = None,
        beatmap_id: Optional[int] = None,
        version_name: Optional[str] = None,
    ) -> Optional[str]:
        """Smart audio resolver: explicit path -> same folder -> user's osu! Songs directory (set ID -> title/artist)."""
        if self.explicit_audio_path and os.path.isfile(self.explicit_audio_path):
            return self.explicit_audio_path

        # 1. Same folder as beatmap
        folder = os.path.dirname(os.path.abspath(bm_path))
        if audio_filename:
            cand = os.path.join(folder, audio_filename)
            if os.path.isfile(cand):
                return cand

        if os.path.isdir(folder):
            for f in os.listdir(folder):
                if f.lower().endswith((".mp3", ".ogg", ".wav")) and not any(k in f.lower() for k in ("hit", "slider", "spin")):
                    full = os.path.join(folder, f)
                    if os.path.getsize(full) > 200000:
                        return full

        songs_dir = (
            os.path.join(self.osu_dir, "Songs")
            if self.osu_dir and os.path.isdir(os.path.join(self.osu_dir, "Songs"))
            else os.path.join(os.path.expanduser("~"), "AppData", "Local", "osu!", "Songs")
        )
        if not os.path.isdir(songs_dir):
            return None

        # 2. Priority 1 for Songs folder: match by BeatmapSetID (exact folder name or prefix)
        if beatmapset_id and beatmapset_id > 0:
            sid_str = str(beatmapset_id)
            for sdir in os.listdir(songs_dir):
                if sdir == sid_str or sdir.startswith(f"{sid_str} ") or sdir.startswith(f"{sid_str}_"):
                    cand_folder = os.path.join(songs_dir, sdir)
                    if os.path.isdir(cand_folder):
                        if audio_filename:
                            cand = os.path.join(cand_folder, audio_filename)
                            if os.path.isfile(cand):
                                return cand
                        for f in os.listdir(cand_folder):
                            if f.lower().endswith((".mp3", ".ogg", ".wav")) and not any(k in f.lower() for k in ("hit", "slider", "spin")):
                                full = os.path.join(cand_folder, f)
                                if os.path.getsize(full) > 200000:
                                    return full

        # 3. Match by Title / Artist with verification of .osu content
        clean_title = re.sub(r"[^a-zA-Z0-9]", "", title).lower()
        clean_artist = re.sub(r"[^a-zA-Z0-9]", "", artist).lower()

        best_folder = None
        best_score = 0
        for sdir in os.listdir(songs_dir):
            cand_folder = os.path.join(songs_dir, sdir)
            if not os.path.isdir(cand_folder):
                continue
            sd_clean = re.sub(r"[^a-zA-Z0-9]", "", sdir).lower()
            score = 0
            if clean_title and len(clean_title) >= 4 and clean_title in sd_clean:
                score += 30
            elif clean_title and len(clean_title) >= 4 and clean_title[:8] in sd_clean:
                score += 10
            if clean_artist and len(clean_artist) >= 3 and clean_artist in sd_clean:
                score += 15
            elif clean_artist and len(clean_artist) >= 3 and clean_artist[:6] in sd_clean:
                score += 5

            # If score is good, verify if this folder contains matching .osu
            if score >= 15 and version_name:
                try:
                    for ofile in os.listdir(cand_folder):
                        if ofile.endswith(".osu") and version_name.lower() in ofile.lower():
                            score += 50
                            break
                except Exception:
                    pass

            if score > best_score:
                best_score = score
                best_folder = cand_folder

        if best_folder and best_score >= 15:
            if audio_filename:
                cand = os.path.join(best_folder, audio_filename)
                if os.path.isfile(cand):
                    return cand
            for f in os.listdir(best_folder):
                if f.lower().endswith((".mp3", ".ogg", ".wav")) and not any(k in f.lower() for k in ("hit", "slider", "spin")):
                    full = os.path.join(best_folder, f)
                    if os.path.getsize(full) > 200000:
                        return full
        return None

    def setup_audio_for_beatmap(self):
        """Configure and load audio track and hitsounds for the loaded beatmap."""
        if self.audio_available:
            try:
                pygame.mixer.music.stop()
            except Exception:
                pass
        self.music_loaded = False
        self.music_playing = False
        self.audio_path = None

        self.build_sound_events()

        if not self.audio_available or self.disable_audio_playback:
            self.audio_anchor_track_ms = self.current_time_ms
            self.audio_anchor_perf_time = time.perf_counter()
            return

        audio_file = self.resolve_audio_file(
            self.beatmap_path,
            getattr(self.beatmap, "audio_filename", ""),
            getattr(self.beatmap, "artist", ""),
            getattr(self.beatmap, "title", ""),
            beatmapset_id=getattr(self.beatmap, "beatmapset_id", None),
            beatmap_id=getattr(self.beatmap, "beatmap_id", None),
            version_name=getattr(self.beatmap, "version_name", None),
        )

        if audio_file and os.path.isfile(audio_file):
            try:
                pygame.mixer.music.load(audio_file)
                pygame.mixer.music.set_volume(0.0 if self.audio_muted else self.music_volume)
                self.music_loaded = True
                self.audio_path = audio_file
            except Exception:
                self.music_loaded = False
                self.audio_path = None

        self.audio_anchor_track_ms = self.current_time_ms
        self.audio_anchor_perf_time = time.perf_counter()

        lead_in = getattr(self.beatmap, "audio_lead_in", 0)
        if self.is_playing and self.music_loaded and self.current_time_ms >= lead_in and self.playback_speed == 1.0:
            target_sec = max(0.0, (self.current_time_ms - lead_in + self.audio_offset_ms) / 1000.0)
            try:
                pygame.mixer.music.play(start=target_sec)
                try:
                    pygame.mixer.music.set_pos(target_sec)
                except Exception:
                    pass
                self.music_playing = True
            except Exception:
                pass

    def build_sound_events(self):
        """Extract and sort note hitsound trigger timestamps."""
        self.sound_events = []
        if not self.beatmap or not self.beatmap.hit_objects:
            self.sound_event_idx = 0
            return

        for obj in self.beatmap.hit_objects:
            sound_kind = self._sound_type_for(getattr(obj, "hit_sound", 0))
            if obj.kind == "circle":
                self.sound_events.append((obj.time, sound_kind))
            elif obj.kind == "slider":
                self.sound_events.append((obj.time, sound_kind))
                spans = getattr(obj, "spans", 1)
                if spans and spans > 1:
                    span_dur = (obj.end_time - obj.time) / spans
                    for s in range(1, spans):
                        self.sound_events.append((obj.time + s * span_dur, "normal"))
                self.sound_events.append((obj.end_time, "normal"))
            elif obj.kind == "spinner":
                self.sound_events.append((obj.end_time, "finish"))

        self.sound_events.sort(key=lambda x: x[0])
        times = [x[0] for x in self.sound_events]
        self.sound_event_idx = bisect.bisect_left(times, self.current_time_ms)

    @staticmethod
    def _sound_type_for(mask: int) -> str:
        if mask & 4:
            return "finish"
        elif mask & 8:
            return "clap"
        elif mask & 2:
            return "whistle"
        return "normal"

    def play_hitsound(self, kind: str):
        if self.audio_muted or not self.hitsounds_enabled or not self.audio_available:
            return
        snd = self.hitsound_samples.get(kind) or self.hitsound_samples.get("normal")
        if snd and getattr(snd, "get_length", lambda: 1.0)() >= 0.005:
            try:
                chan = pygame.mixer.find_channel()
                if chan:
                    chan.play(snd)
                else:
                    snd.play()
            except Exception:
                pass

    def toggle_audio_mute(self):
        """Toggle audio mute state (key U)."""
        self.audio_muted = not self.audio_muted
        if self.audio_muted:
            try:
                pygame.mixer.music.set_volume(0.0)
            except Exception:
                pass
            self.set_toast("Звук выключен (Mute)", 1.5)
        else:
            try:
                pygame.mixer.music.set_volume(self.music_volume)
            except Exception:
                pass
            self.set_toast(f"Звук включен ({int(self.music_volume * 100)}%)", 1.5)

    def adjust_audio_offset(self, delta_ms: float):
        """Adjust universal audio offset (Alt + Left/Right arrows)."""
        self.audio_offset_ms += delta_ms
        self.set_toast(f"Смещение звука: {self.audio_offset_ms:+.1f} мс", 1.5)
        lead_in = getattr(self.beatmap, "audio_lead_in", 0)
        if self.music_playing and self.playback_speed == 1.0 and self.current_time_ms >= lead_in:
            target_sec = max(0.0, (self.current_time_ms - lead_in + self.audio_offset_ms) / 1000.0)
            try:
                pygame.mixer.music.play(start=target_sec)
                try:
                    pygame.mixer.music.set_pos(target_sec)
                except Exception:
                    pass
                self.audio_anchor_track_ms = self.current_time_ms
                self.audio_anchor_perf_time = time.perf_counter()
            except Exception:
                pass

    def toggle_playback(self):
        """Toggle pause/play with synchronized audio."""
        self.is_playing = not self.is_playing
        if not self.is_playing:
            if self.music_playing:
                try:
                    pygame.mixer.music.pause()
                except Exception:
                    pass
        else:
            self.audio_anchor_track_ms = self.current_time_ms
            self.audio_anchor_perf_time = time.perf_counter()
            lead_in = getattr(self.beatmap, "audio_lead_in", 0)
            if self.music_loaded and self.current_time_ms >= lead_in and self.playback_speed == 1.0:
                if self.music_playing:
                    try:
                        pygame.mixer.music.unpause()
                    except Exception:
                        pass
                else:
                    target_sec = max(0.0, (self.current_time_ms - lead_in + self.audio_offset_ms) / 1000.0)
                    try:
                        pygame.mixer.music.play(start=target_sec)
                        try:
                            pygame.mixer.music.set_pos(target_sec)
                        except Exception:
                            pass
                        self.music_playing = True
                    except Exception:
                        pass

    def change_speed(self, delta: float):
        """
        Smoothly adjust playback speed in range [0.01, 2.00] with minimum step 0.01.
        At non-1.00x speeds, background music track is paused to avoid audio distortion,
        while hitsounds continue clicking in perfect sync at the new speed.
        """
        old_speed = self.playback_speed
        new_speed = round(max(0.01, min(2.00, self.playback_speed + delta)), 2)
        if new_speed == old_speed:
            return

        self.playback_speed = new_speed
        self.audio_anchor_track_ms = self.current_time_ms
        self.audio_anchor_perf_time = time.perf_counter()

        if self.playback_speed != 1.0:
            if self.music_playing:
                try:
                    pygame.mixer.music.pause()
                except Exception:
                    pass
                self.music_playing = False
            self.set_toast(f"Скорость {self.playback_speed:.2f}x (музыка заглушена)", 1.0)
        else:
            lead_in = getattr(self.beatmap, "audio_lead_in", 0)
            if self.music_loaded and self.current_time_ms >= lead_in and self.is_playing:
                target_sec = max(0.0, (self.current_time_ms - lead_in + self.audio_offset_ms) / 1000.0)
                try:
                    pygame.mixer.music.play(start=target_sec)
                    try:
                        pygame.mixer.music.set_pos(target_sec)
                    except Exception:
                        pass
                    self.music_playing = True
                except Exception:
                    pass
            self.set_toast("Скорость 1.00x (музыка в синхроне)", 1.2)

    def set_speed(self, speed: float):
        """Directly set playback speed (e.g. reset to 1.00x)."""
        delta = speed - self.playback_speed
        self.change_speed(delta)

    def skip_intro(self):
        """Skip beatmap intro to just before the first note appears."""
        if not self.beatmap or not self.beatmap.hit_objects:
            return
        first_time = self.beatmap.hit_objects[0].time
        preempt, _ = self.beatmap.ar_preempt_fadein
        target = max(0.0, first_time - preempt - 600.0)
        if self.current_time_ms < target - 500.0:
            self.seek_to(target)
            self.set_toast(f"⏭ Интро пропущено ({target/1000.0:.2f}с)", 1.5)

    def can_skip_intro(self) -> bool:
        """Check if intro skip is currently available."""
        if not self.beatmap or not self.beatmap.hit_objects:
            return False
        first_time = self.beatmap.hit_objects[0].time
        preempt, _ = self.beatmap.ar_preempt_fadein
        target = max(0.0, first_time - preempt - 600.0)
        return self.current_time_ms < target - 800.0 and target > 2500.0

    def init_fonts(self):
        try:
            self.font_main = pygame.font.SysFont("Segoe UI, Arial, sans-serif", 18)
            self.font_title = pygame.font.SysFont("Segoe UI, Arial, sans-serif", 22, bold=True)
            self.font_bold = pygame.font.SysFont("Segoe UI, Arial, sans-serif", 16, bold=True)
            self.font_small = pygame.font.SysFont("Segoe UI, Arial, sans-serif", 13)
            self.font_combo = pygame.font.SysFont("Segoe UI, Arial, sans-serif", 20, bold=True)
            self.font_spinner = pygame.font.SysFont("Segoe UI, Arial, sans-serif", 36, bold=True)
            self.font_click = pygame.font.SysFont("Segoe UI, Arial, sans-serif", 13, bold=True)
        except Exception:
            self.font_main = pygame.font.Font(None, 20)
            self.font_title = pygame.font.Font(None, 24)
            self.font_bold = pygame.font.Font(None, 18)
            self.font_small = pygame.font.Font(None, 14)
            self.font_combo = pygame.font.Font(None, 22)
            self.font_spinner = pygame.font.Font(None, 40)
            self.font_click = pygame.font.Font(None, 14)

    def update_playfield_layout(self):
        """Calculate playfield scaling and position with 512x384 aspect ratio."""
        hud_h = 70
        timeline_h = 60
        total_avail_h = max(200, self.height - hud_h - timeline_h)

        if self.show_tech_hud:
            # Dedicated side dock width
            panel_w = min(400, max(330, int(self.width * 0.35)))
            panel_h = total_avail_h - 14
            panel_y = hud_h + 7

            margin = 14
            avail_w = max(200, self.width - panel_w - margin * 3)
            avail_h = total_avail_h * 0.94

            scale_x = avail_w / 512.0
            scale_y = avail_h / 384.0
            self.scale = min(scale_x, scale_y)

            pf_w = 512.0 * self.scale
            pf_h = 384.0 * self.scale

            remaining_w = self.width - panel_w - margin * 2

            if self.tech_hud_dock == "left":
                panel_x = margin
                self.offset_x = (panel_w + margin) + (remaining_w - pf_w) / 2.0
            else:
                panel_x = self.width - panel_w - margin
                self.offset_x = margin + (remaining_w - pf_w) / 2.0

            self.tech_panel_rect = pygame.Rect(panel_x, panel_y, panel_w, panel_h)
            self.offset_y = hud_h + (total_avail_h - pf_h) / 2.0
        else:
            avail_w = self.width * 0.92
            avail_h = total_avail_h * 0.92

            scale_x = avail_w / 512.0
            scale_y = avail_h / 384.0
            self.scale = min(scale_x, scale_y)

            pf_w = 512.0 * self.scale
            pf_h = 384.0 * self.scale
            self.offset_x = (self.width - pf_w) / 2.0
            self.offset_y = hud_h + (total_avail_h - pf_h) / 2.0
            self.tech_panel_rect = pygame.Rect(0, 0, 0, 0)

        self.pf_rect = pygame.Rect(
            int(self.offset_x), int(self.offset_y), int(pf_w), int(pf_h)
        )
        self.screen_radius = max(8.0, self.beatmap.circle_radius * self.scale)

    def osu_to_screen(self, x: float, y: float) -> Tuple[int, int]:
        return int(self.offset_x + x * self.scale), int(self.offset_y + y * self.scale)

    def active_break_at(self, time_ms: float) -> Optional[Tuple[float, float]]:
        """Return break period (start_time, end_time) containing time_ms, if any."""
        for b_start, b_end in getattr(self.beatmap, "breaks", []):
            if b_start <= time_ms <= b_end:
                return (b_start, b_end)
        return None

    def build_slider_caches(self):
        """Pre-render slider paths to surfaces for high performance."""
        self.slider_caches.clear()
        r = self.screen_radius
        pad = int(r + 6)
        combo_colours = self.beatmap.combo_colours or [(255, 192, 0)]
        border_rgb = getattr(self.beatmap, "slider_border", None) or (220, 220, 220)
        track_override = getattr(self.beatmap, "slider_track_override", None)

        for obj in self.beatmap.hit_objects:
            if obj.kind != "slider" or not obj.path_points:
                continue

            # Compute screen coords of path points
            pts = [self.osu_to_screen(p[0], p[1]) for p in obj.path_points]
            xs = [p[0] for p in pts]
            ys = [p[1] for p in pts]
            min_x, max_x = min(xs) - pad, max(xs) + pad
            min_y, max_y = min(ys) - pad, max(ys) + pad
            surf_w = max(10, max_x - min_x)
            surf_h = max(10, max_y - min_y)

            surf = pygame.Surface((surf_w, surf_h), pygame.SRCALPHA)
            combo_col = combo_colours[obj.combo_index % len(combo_colours)]

            # Relative points
            rel_pts = [(p[0] - min_x, p[1] - min_y) for p in pts]

            # Step along path to stamp circles
            step = max(1, int(r * 0.2))
            border_rgba = (border_rgb[0], border_rgb[1], border_rgb[2], 225)
            for i in range(0, len(rel_pts), step):
                pt = rel_pts[i]
                pygame.draw.circle(surf, border_rgba, pt, int(r))
            if rel_pts:
                pygame.draw.circle(surf, border_rgba, rel_pts[-1], int(r))

            # Inner track (uses [Colours] SliderTrackOverride when present)
            track_rgba = (
                (track_override[0], track_override[1], track_override[2], 230)
                if track_override is not None
                else (40, 40, 48, 230)
            )
            for i in range(0, len(rel_pts), step):
                pt = rel_pts[i]
                pygame.draw.circle(surf, track_rgba, pt, int(r - 3.5))
            if rel_pts:
                pygame.draw.circle(surf, track_rgba, rel_pts[-1], int(r - 3.5))

            # Inner colored tint from combo colour unless SliderTrackOverride is active
            if track_override is None:
                for i in range(0, len(rel_pts), step * 2):
                    pt = rel_pts[i]
                    pygame.draw.circle(
                        surf,
                        (combo_col[0], combo_col[1], combo_col[2], 90),
                        pt,
                        int(r * 0.55),
                    )

            self.slider_caches[obj.id] = (surf, min_x, min_y)

    def seek_to(self, time_ms: float):
        self.current_time_ms = max(self.start_time, min(self.last_time, time_ms))
        self.audio_anchor_track_ms = self.current_time_ms
        self.audio_anchor_perf_time = time.perf_counter()

        if self.live_clicks:
            self.live_clicks = [c for c in self.live_clicks if c.time_ms <= self.current_time_ms]

        if self.sound_events:
            times = [x[0] for x in self.sound_events]
            self.sound_event_idx = bisect.bisect_left(times, self.current_time_ms)

        if self.music_loaded:
            lead_in = getattr(self.beatmap, "audio_lead_in", 0)
            if self.current_time_ms >= lead_in and self.playback_speed == 1.0:
                target_sec = max(0.0, (self.current_time_ms - lead_in + self.audio_offset_ms) / 1000.0)
                try:
                    pygame.mixer.music.play(start=target_sec)
                    try:
                        pygame.mixer.music.set_pos(target_sec)
                    except Exception:
                        pass
                    if not self.is_playing:
                        pygame.mixer.music.pause()
                        self.music_playing = False
                    else:
                        self.music_playing = True
                except Exception:
                    pass
            else:
                try:
                    pygame.mixer.music.stop()
                except Exception:
                    pass
                self.music_playing = False

    def step(self, dt_ms: float):
        if not self.is_playing:
            self.audio_anchor_perf_time = time.perf_counter()
            return

        if self.headless or not self.music_playing:
            self.current_time_ms += dt_ms * self.playback_speed
            self.audio_anchor_track_ms = self.current_time_ms
            self.audio_anchor_perf_time = time.perf_counter()
        else:
            perf_now = time.perf_counter()
            delta_ms = (perf_now - self.audio_anchor_perf_time) * 1000.0 * self.playback_speed
            self.current_time_ms = self.audio_anchor_track_ms + delta_ms

            # Hardware audio drift correction & track completion check
            if self.playback_speed == 1.0:
                if not pygame.mixer.music.get_busy():
                    self.music_playing = False
                else:
                    hw_pos = pygame.mixer.music.get_pos()
                    if hw_pos >= 0:
                        expected_time = self.audio_anchor_track_ms + hw_pos
                        drift = self.current_time_ms - expected_time
                        if abs(drift) > 20.0:
                            self.audio_anchor_perf_time += (drift / 1000.0) * 0.1

        # Start audio when crossing lead_in ms in interactive mode
        lead_in = getattr(self.beatmap, "audio_lead_in", 0)
        if not self.headless and self.music_loaded and not self.music_playing and self.current_time_ms >= lead_in and self.playback_speed == 1.0:
            target_sec = max(0.0, (self.current_time_ms - lead_in + self.audio_offset_ms) / 1000.0)
            try:
                pygame.mixer.music.play(start=target_sec)
                try:
                    pygame.mixer.music.set_pos(target_sec)
                except Exception:
                    pass
                self.music_playing = True
                self.audio_anchor_track_ms = self.current_time_ms
                self.audio_anchor_perf_time = time.perf_counter()
            except Exception:
                pass

        # Trigger hitsounds
        if not self.audio_muted and self.hitsounds_enabled and self.sound_events:
            while self.sound_event_idx < len(self.sound_events):
                ev_time, ev_kind = self.sound_events[self.sound_event_idx]
                if ev_time <= self.current_time_ms:
                    if self.current_time_ms - ev_time <= 80.0:
                        self.play_hitsound(ev_kind)
                    self.sound_event_idx += 1
                else:
                    break

        if self.current_time_ms > self.last_time:
            self.seek_to(self.start_time)

    def render(self):
        self.screen.fill((16, 17, 24))

        # Draw Playfield Background and Border
        pygame.draw.rect(self.screen, (24, 26, 36), self.pf_rect)
        pygame.draw.rect(self.screen, (70, 75, 95), self.pf_rect, 2)

        # Coordinate crosshair in center (256, 192)
        cx, cy = self.osu_to_screen(256, 192)
        pygame.draw.line(self.screen, (40, 43, 56), (cx - 20, cy), (cx + 20, cy), 1)
        pygame.draw.line(self.screen, (40, 43, 56), (cx, cy - 20), (cx, cy + 20), 1)

        # Active Break Period indicator on playfield
        active_br = self.active_break_at(self.current_time_ms)
        if active_br is not None and active_br[1] > active_br[0]:
            rem_sec = max(0.0, (active_br[1] - self.current_time_ms) / 1000.0)
            br_surf = self.font_bold.render(f"BREAK  {rem_sec:.1f}s", True, (94, 234, 212))
            bw = br_surf.get_width() + 20
            bh = 24
            bx = self.pf_rect.x + (self.pf_rect.width - bw) // 2
            by = self.pf_rect.y + 10
            pill = pygame.Surface((bw, bh), pygame.SRCALPHA)
            pygame.draw.rect(pill, (20, 48, 58, 210), (0, 0, bw, bh), border_radius=5)
            pygame.draw.rect(pill, (94, 212, 224, 190), (0, 0, bw, bh), 1, border_radius=5)
            pill.blit(br_surf, (10, (bh - br_surf.get_height()) // 2))
            self.screen.blit(pill, (bx, by))

        # Render Hit Objects
        self.render_hit_objects()

        # Render Replay Cursor if active
        if self.replay and self.show_replay:
            self.render_replay_cursor()

        # Render Recent Click Badges on Playfield (Technical Inspector mode)
        if self.show_tech_hud:
            self.render_recent_clicks()

        # Render HUD & Diagnostics
        if self.show_hud:
            self.render_hud()

        # Render Technical Inspector HUD if toggled
        if self.show_tech_hud:
            self.render_tech_hud()

        # Render Timeline Seekbar
        self.render_timeline()

        # Render Toast notification
        if self.toast_message and time.time() < self.toast_until:
            self.render_toast()

        # Render Verification Details if toggled
        if self.show_verification_detail:
            self.render_verification_overlay()

    def render_replay_cursor(self):
        pos, keys = self.replay.cursor_at(self.current_time_ms)
        if pos is None:
            return
        sx, sy = self.osu_to_screen(pos[0], pos[1])

        # Update trail
        self.replay_trail.append((sx, sy))
        if len(self.replay_trail) > 12:
            self.replay_trail.pop(0)

        n_trail = len(self.replay_trail)
        for idx, (tx, ty) in enumerate(self.replay_trail):
            frac = (idx + 1) / n_trail
            alpha = int(frac * 160)
            radius = max(2, int(frac * 5))
            trail_surf = pygame.Surface(
                (radius * 2 + 2, radius * 2 + 2), pygame.SRCALPHA
            )
            pygame.draw.circle(
                trail_surf, (255, 220, 90, alpha), (radius + 1, radius + 1), radius
            )
            self.screen.blit(trail_surf, (tx - radius - 1, ty - radius - 1))

        # Cursor ring (lights yellow when any key is pressed)
        is_clicking = bool(keys & 15)
        ring_col = (255, 235, 80) if is_clicking else (255, 255, 255)
        pygame.draw.circle(self.screen, ring_col, (sx, sy), 11, 2)
        pygame.draw.circle(self.screen, (255, 170, 40), (sx, sy), 5)

    def render_key_overlay(self):
        keys = 0
        if self.replay:
            _, keys = self.replay.cursor_at(self.current_time_ms)
        pressed = pygame.key.get_pressed()
        m_pressed = pygame.mouse.get_pressed()
        k1_active = bool(keys & 4) or pressed[pygame.K_z] or pressed[pygame.K_d]
        k2_active = bool(keys & 8) or pressed[pygame.K_x] or pressed[pygame.K_f]
        m1_active = bool(keys & 1) or m_pressed[0]
        m2_active = bool(keys & 2) or m_pressed[2]
        key_states = [
            ("K1", k1_active),
            ("K2", k2_active),
            ("M1", m1_active),
            ("M2", m2_active),
        ]
        kw, kh = 32, 28
        total_keys_w = (kw + 6) * 4 - 6
        if self.show_tech_hud and self.tech_hud_dock == "right":
            start_x = self.tech_panel_rect.x - total_keys_w - 14
        else:
            start_x = self.width - (kw + 6) * 4 - 20
        start_y = 75
        for i, (label, active) in enumerate(key_states):
            kx = start_x + i * (kw + 6)
            bg_col = (255, 180, 50) if active else (35, 38, 52)
            border_col = (255, 220, 100) if active else (70, 75, 100)
            text_col = (20, 20, 20) if active else (180, 185, 200)
            pygame.draw.rect(
                self.screen, bg_col, (kx, start_y, kw, kh), border_radius=4
            )
            pygame.draw.rect(
                self.screen, border_col, (kx, start_y, kw, kh), 1, border_radius=4
            )
            t_surf = self.font_bold.render(label, True, text_col)
            self.screen.blit(
                t_surf, t_surf.get_rect(center=(kx + kw // 2, start_y + kh // 2))
            )

    def precompute_replay_clicks(self):
        """Extract and evaluate all key press clicks in replay against beatmap hit objects."""
        if not self.replay or not self.replay.frames or not self.beatmap:
            self.replay_clicks = []
            self.replay_click_times = []
            return

        od = getattr(self.beatmap, "od", 8.0)
        cs = getattr(self.beatmap, "cs", 4.0)
        w300 = 80.0 - 6.0 * od
        w100 = 140.0 - 8.0 * od
        w50 = 200.0 - 10.0 * od
        circle_r = 54.4 - 4.48 * cs

        raw_clicks = []
        prev_keys = 0
        for frame in self.replay.frames:
            curr_keys = frame.keys
            for bit, name in [(4, "K1"), (8, "K2"), (1, "M1"), (2, "M2")]:
                if (curr_keys & bit) and not (prev_keys & bit):
                    # Prefer K1 / K2 if keyboard set mouse bit as alias
                    if bit == 1 and (curr_keys & 4):
                        continue
                    if bit == 2 and (curr_keys & 8):
                        continue
                    raw_clicks.append((frame.time_ms, frame.x, frame.y, name))
            prev_keys = curr_keys

        events: List[ClickEvent] = []
        hit_objects = [
            obj for obj in self.beatmap.hit_objects
            if obj.kind in ("circle", "slider")
        ]
        used_objects = set()

        for click_time, cx, cy, key in raw_clicks:
            best_idx = None
            best_delta = 999999.0
            best_dist = 999999.0

            for idx, obj in enumerate(hit_objects):
                if idx in used_objects:
                    continue
                delta = click_time - obj.time
                if delta < -w50 - 60.0:
                    break
                if -w50 <= delta <= w50:
                    obj_x = getattr(obj, "stacked_x", obj.x)
                    obj_y = getattr(obj, "stacked_y", obj.y)
                    dist = math.hypot(cx - obj_x, cy - obj_y)
                    if dist <= circle_r:
                        if best_idx is None or abs(delta) < abs(best_delta):
                            best_idx = idx
                            best_delta = delta
                            best_dist = dist
                    elif best_idx is None:
                        if abs(delta) < abs(best_delta):
                            best_delta = delta
                            best_dist = dist

            if best_idx is not None:
                if abs(best_delta) <= w300:
                    res = "300"
                    col = (56, 189, 248)
                elif abs(best_delta) <= w100:
                    res = "100"
                    col = (74, 222, 128)
                elif abs(best_delta) <= w50:
                    res = "50"
                    col = (251, 146, 60)
                else:
                    res = "Miss"
                    col = (248, 113, 113)
                used_objects.add(best_idx)
                sign = "+" if best_delta >= 0 else ""
                badge_text = f"{key} ({sign}{best_delta:.1f}ms {res})"
                events.append(ClickEvent(click_time, cx, cy, key, best_delta, res, col, badge_text))
            else:
                near_delta = None
                for idx, obj in enumerate(hit_objects):
                    if idx in used_objects:
                        continue
                    d = click_time - obj.time
                    if near_delta is None or abs(d) < abs(near_delta):
                        near_delta = d
                if near_delta is not None and abs(near_delta) <= 300.0:
                    sign = "+" if near_delta >= 0 else ""
                    badge_text = f"{key} ({sign}{near_delta:.1f}ms Miss)"
                    events.append(ClickEvent(click_time, cx, cy, key, near_delta, "Miss", (248, 113, 113), badge_text))
                else:
                    badge_text = f"{key} (Miss)"
                    events.append(ClickEvent(click_time, cx, cy, key, None, "Miss", (248, 113, 113), badge_text))

        self.replay_clicks = events
        self.replay_click_times = [e.time_ms for e in events]

    def record_live_click(self, key_name: str):
        """Record an interactive user click (Z, X, LMB, RMB) with timing judgement."""
        if not self.beatmap:
            return
        t = self.current_time_ms
        mx, my = pygame.mouse.get_pos()
        if self.pf_rect.collidepoint(mx, my):
            cx = (mx - self.offset_x) / self.scale
            cy = (my - self.offset_y) / self.scale
        elif self.replay:
            pos, _ = self.replay.cursor_at(t)
            cx, cy = pos if pos else (256.0, 192.0)
        else:
            cx = (mx - self.offset_x) / self.scale
            cy = (my - self.offset_y) / self.scale

        od = getattr(self.beatmap, "od", 8.0)
        cs = getattr(self.beatmap, "cs", 4.0)
        w300 = 80.0 - 6.0 * od
        w100 = 140.0 - 8.0 * od
        w50 = 200.0 - 10.0 * od
        circle_r = 54.4 - 4.48 * cs

        best_obj = None
        best_delta = 999999.0
        for obj in self.beatmap.hit_objects:
            if obj.kind in ("circle", "slider"):
                delta = t - obj.time
                if -w50 <= delta <= w50:
                    obj_x = getattr(obj, "stacked_x", obj.x)
                    obj_y = getattr(obj, "stacked_y", obj.y)
                    dist = math.hypot(cx - obj_x, cy - obj_y)
                    if dist <= circle_r:
                        if best_obj is None or abs(delta) < abs(best_delta):
                            best_obj = obj
                            best_delta = delta

        if best_obj is not None:
            if abs(best_delta) <= w300:
                res = "300"
                col = (56, 189, 248)
            elif abs(best_delta) <= w100:
                res = "100"
                col = (74, 222, 128)
            elif abs(best_delta) <= w50:
                res = "50"
                col = (251, 146, 60)
            else:
                res = "Miss"
                col = (248, 113, 113)
            sign = "+" if best_delta >= 0 else ""
            badge_text = f"{key_name} ({sign}{best_delta:.1f}ms {res})"
            ev = ClickEvent(t, cx, cy, key_name, best_delta, res, col, badge_text)
        else:
            ev = ClickEvent(t, cx, cy, key_name, None, "Miss", (248, 113, 113), f"{key_name} (Miss)")

        self.live_clicks.append(ev)

    def get_latest_click_at(self, t: float) -> Optional[ClickEvent]:
        """Find the latest click event that occurred at or before time t within 3000ms."""
        latest: Optional[ClickEvent] = None
        if self.replay_clicks and self.replay_click_times:
            idx = bisect.bisect_right(self.replay_click_times, t)
            if idx > 0:
                c = self.replay_clicks[idx - 1]
                if t - c.time_ms <= 3000.0:
                    latest = c
        for c in self.live_clicks:
            if c.time_ms <= t and (t - c.time_ms <= 3000.0):
                if latest is None or c.time_ms > latest.time_ms:
                    latest = c
        return latest

    def render_recent_clicks(self):
        """Render floating badges on the playfield for up to the last 5 clicks when Technical HUD is active."""
        if not self.show_tech_hud:
            return

        t = self.current_time_ms
        active_clicks: List[ClickEvent] = []

        # Replay clicks up to current_time_ms
        if self.replay_clicks and self.replay_click_times:
            idx = bisect.bisect_right(self.replay_click_times, t)
            for c in reversed(self.replay_clicks[:idx]):
                age = t - c.time_ms
                if age > 2500.0:
                    break
                active_clicks.append(c)

        # Interactive live clicks
        for c in self.live_clicks:
            age = t - c.time_ms
            if 0.0 <= age <= 2500.0:
                active_clicks.append(c)

        if not active_clicks:
            return

        # Sort chronologically and keep only up to the last 5 clicks
        active_clicks.sort(key=lambda c: c.time_ms)
        visible_clicks = active_clicks[-5:]
        n = len(visible_clicks)

        for rank, click in enumerate(visible_clicks):
            age = max(0.0, t - click.time_ms)
            recency_alpha = 0.55 + 0.45 * ((rank + 1) / n)
            time_alpha = 1.0 if age <= 1500.0 else max(0.0, (2500.0 - age) / 1000.0)
            overall_alpha = min(1.0, max(0.0, recency_alpha * time_alpha))
            if overall_alpha <= 0.02:
                continue

            sx, sy = self.osu_to_screen(click.x, click.y)
            self._render_single_click_badge(click, sx, sy, overall_alpha, age)

    def _render_single_click_badge(
        self, click: ClickEvent, sx: int, sy: int, alpha: float, age: float
    ):
        """Render a single click hitpoint and floating pill badge at (sx, sy)."""
        # 1. Hit dot and ripple at exact click point (sx, sy)
        dot_surf = pygame.Surface((36, 36), pygame.SRCALPHA)
        center = (18, 18)
        col = click.color

        # Inner dot
        pygame.draw.circle(
            dot_surf,
            (col[0], col[1], col[2], int(230 * alpha)),
            center,
            4,
        )
        # Small white center pip
        pygame.draw.circle(
            dot_surf,
            (255, 255, 255, int(220 * alpha)),
            center,
            2,
        )
        # Outer ring
        pygame.draw.circle(
            dot_surf,
            (col[0], col[1], col[2], int(160 * alpha)),
            center,
            7,
            1,
        )
        # Expansion ripple during initial 220ms
        if age < 220.0:
            rip_prog = age / 220.0
            rip_r = int(7 + rip_prog * 9)
            rip_alpha = int((1.0 - rip_prog) * 200 * alpha)
            if rip_alpha > 0 and rip_r <= 17:
                pygame.draw.circle(
                    dot_surf,
                    (col[0], col[1], col[2], rip_alpha),
                    center,
                    rip_r,
                    1,
                )
        self.screen.blit(dot_surf, (sx - 18, sy - 18))

        # 2. Prepare badge text
        tag_surf = self.font_click.render(click.key, True, (255, 255, 255))
        tag_w = tag_surf.get_width() + 10
        tag_h = 18

        if click.delta_ms is not None:
            sign = "+" if click.delta_ms >= 0 else ""
            timing_str = f"({sign}{click.delta_ms:.1f}ms "
            timing_surf = self.font_click.render(timing_str, True, (215, 225, 240))
            res_str = f"{click.result})"
            res_surf = self.font_click.render(res_str, True, col)
        else:
            timing_surf = self.font_click.render("(", True, (215, 225, 240))
            res_str = f"{click.result})"
            res_surf = self.font_click.render(res_str, True, col)

        pad_x = 7
        total_w = pad_x * 2 + tag_w + 4 + timing_surf.get_width() + res_surf.get_width()
        total_h = 24

        badge = pygame.Surface((total_w, total_h), pygame.SRCALPHA)
        # Translucent dark body
        pygame.draw.rect(
            badge,
            (16, 19, 28, int(225 * alpha)),
            (0, 0, total_w, total_h),
            border_radius=5,
        )
        # Accent border colored by judgement
        pygame.draw.rect(
            badge,
            (col[0], col[1], col[2], int(190 * alpha)),
            (0, 0, total_w, total_h),
            1,
            border_radius=5,
        )

        # Key Tag Box
        tag_box = pygame.Rect(pad_x, (total_h - tag_h) // 2, tag_w, tag_h)
        pygame.draw.rect(
            badge,
            (35, 42, 60, int(240 * alpha)),
            tag_box,
            border_radius=4,
        )
        pygame.draw.rect(
            badge,
            (85, 100, 135, int(180 * alpha)),
            tag_box,
            1,
            border_radius=4,
        )
        badge.blit(tag_surf, tag_surf.get_rect(center=tag_box.center))

        # Timing and Judgement text
        tx = pad_x + tag_w + 4
        ty = (total_h - timing_surf.get_height()) // 2
        badge.blit(timing_surf, (tx, ty))
        tx += timing_surf.get_width()
        badge.blit(res_surf, (tx, ty))

        # Set overall alpha for the badge
        badge.set_alpha(int(255 * alpha))

        # 3. Position badge
        bx = int(sx - total_w // 2)
        by = int(sy - total_h - 6)
        if by < self.pf_rect.y + 4:
            by = int(sy + 12)

        # Clamp inside playfield bounds
        bx = max(self.pf_rect.x + 4, min(self.pf_rect.right - total_w - 4, bx))
        by = max(self.pf_rect.y + 4, min(self.pf_rect.bottom - total_h - 4, by))

        self.screen.blit(badge, (bx, by))

    def render_toast(self):
        rem = self.toast_until - time.time()
        alpha = min(1.0, max(0.0, rem * 2.0))
        t_surf = self.font_bold.render(self.toast_message, True, (255, 255, 255))
        tw = t_surf.get_width() + 28
        th = 36
        tx = (self.width - tw) // 2
        ty = self.height - 95
        surf = pygame.Surface((tw, th), pygame.SRCALPHA)
        surf.fill((25, 28, 40, int(alpha * 240)))
        pygame.draw.rect(
            surf, (100, 150, 255, int(alpha * 255)), (0, 0, tw, th), 1, border_radius=6
        )
        surf.blit(t_surf, (14, 8))
        self.screen.blit(surf, (tx, ty))

    def render_hit_objects(self):
        t = self.current_time_ms
        preempt, fade_in = self.beatmap.ar_preempt_fadein
        r = self.screen_radius

        # Find visible objects in reverse so earlier objects are drawn on top
        visible_objects = []
        for obj in self.beatmap.hit_objects:
            appear_t = obj.time - preempt
            # Fade out time after object ends
            post_t = 250.0
            disappear_t = obj.end_time + post_t

            if appear_t <= t <= disappear_t:
                visible_objects.append(obj)

        # Draw in order of time
        for obj in reversed(visible_objects):
            if obj.kind == "slider":
                self.render_slider(obj, t, preempt, fade_in, r)
            elif obj.kind == "circle":
                self.render_circle(obj, t, preempt, fade_in, r)
            elif obj.kind == "spinner":
                self.render_spinner(obj, t, preempt, fade_in)

    def render_circle(
        self, obj: HitObject, t: float, preempt: float, fade_in: float, r: float
    ):
        sx, sy = self.osu_to_screen(obj.stacked_x, obj.stacked_y)
        combo_colours = self.beatmap.combo_colours or [(255, 192, 0)]
        combo_col = combo_colours[obj.combo_index % len(combo_colours)]

        elapsed = t - (obj.time - preempt)
        alpha = min(1.0, max(0.0, elapsed / fade_in))

        # Hit circle fade out if t > obj.time
        if t > obj.time:
            fade_out_p = (t - obj.time) / 200.0
            alpha = max(0.0, 1.0 - fade_out_p)

        int_alpha = int(alpha * 255)
        if int_alpha <= 0:
            return

        # Circle body
        body_surf = pygame.Surface((int(r * 2 + 6), int(r * 2 + 6)), pygame.SRCALPHA)
        center = (int(r + 3), int(r + 3))

        # White border
        pygame.draw.circle(body_surf, (255, 255, 255, int_alpha), center, int(r))
        # Inner combo body
        pygame.draw.circle(
            body_surf,
            (combo_col[0], combo_col[1], combo_col[2], int_alpha),
            center,
            int(r - 3),
        )
        # Inner accent
        pygame.draw.circle(
            body_surf,
            (min(255, combo_col[0] + 50), min(255, combo_col[1] + 50), min(255, combo_col[2] + 50), int_alpha),
            center,
            int(r * 0.45),
            2,
        )

        # Combo number
        if t <= obj.time:
            num_surf = self.font_combo.render(str(obj.combo_number), True, (255, 255, 255))
            num_rect = num_surf.get_rect(center=center)
            body_surf.blit(num_surf, num_rect)

        self.screen.blit(body_surf, (sx - center[0], sy - center[1]))

        # Approach Circle (shrinks from 3.5x to 1x radius)
        if t < obj.time:
            p = elapsed / preempt
            app_scale = 1.0 + (1.0 - p) * 2.5
            app_r = int(r * app_scale)
            if app_r > r:
                app_surf = pygame.Surface(
                    (app_r * 2 + 4, app_r * 2 + 4), pygame.SRCALPHA
                )
                app_center = (app_r + 2, app_r + 2)
                pygame.draw.circle(
                    app_surf,
                    (combo_col[0], combo_col[1], combo_col[2], int(min(1.0, alpha * 1.5) * 255)),
                    app_center,
                    app_r,
                    2,
                )
                self.screen.blit(
                    app_surf, (sx - app_center[0], sy - app_center[1])
                )

    def render_slider(
        self, obj: HitObject, t: float, preempt: float, fade_in: float, r: float
    ):
        elapsed = t - (obj.time - preempt)
        alpha = min(1.0, max(0.0, elapsed / fade_in))
        if t > obj.end_time:
            fade_out_p = (t - obj.end_time) / 200.0
            alpha = max(0.0, 1.0 - fade_out_p)

        int_alpha = int(alpha * 255)
        if int_alpha <= 0:
            return

        combo_colours = self.beatmap.combo_colours or [(255, 192, 0)]
        combo_col = combo_colours[obj.combo_index % len(combo_colours)]
        border_rgb = getattr(self.beatmap, "slider_border", None) or (220, 220, 220)

        # Blit cached slider track
        cached = self.slider_caches.get(obj.id)
        if cached:
            surf, mx, my = cached
            # Stacking offset
            dx = int((obj.stacked_x - obj.x) * self.scale)
            dy = int((obj.stacked_y - obj.y) * self.scale)
            if int_alpha < 255:
                # Apply alpha
                track_surf = surf.copy()
                track_surf.set_alpha(int_alpha)
                self.screen.blit(track_surf, (mx + dx, my + dy))
            else:
                self.screen.blit(surf, (mx + dx, my + dy))

        # Slider Head and Tail
        head_x, head_y = self.osu_to_screen(obj.stacked_x, obj.stacked_y)
        tail_pos = obj.position_at_progress(1.0)
        tail_x, tail_y = self.osu_to_screen(tail_pos[0], tail_pos[1])

        # Tail circle
        pygame.draw.circle(self.screen, border_rgb, (tail_x, tail_y), int(r), 2)

        # Repeat arrows if spans > 1
        if obj.spans > 1:
            # Head repeat arrow if odd remaining spans
            # Tail repeat arrow if even remaining spans
            arrow_color = (255, 230, 80)
            pygame.draw.circle(self.screen, arrow_color, (tail_x, tail_y), int(r * 0.45), 2)
            if obj.spans > 2:
                pygame.draw.circle(self.screen, arrow_color, (head_x, head_y), int(r * 0.45), 2)

        # Head circle
        self.render_circle(obj, t, preempt, fade_in, r)

        # Slider Ball and Follow Ring when slider is active
        if obj.time <= t <= obj.end_time:
            active_elapsed = t - obj.time
            span_idx = int(active_elapsed / obj.span_duration) if obj.span_duration > 0 else 0
            span_idx = min(span_idx, obj.spans - 1)
            span_prog = (
                (active_elapsed - span_idx * obj.span_duration) / obj.span_duration
                if obj.span_duration > 0
                else 0.0
            )

            # Reversing direction on odd spans
            curve_prog = 1.0 - span_prog if (span_idx % 2 == 1) else span_prog

            bx, by = obj.position_at_progress(curve_prog)
            bsx, bsy = self.osu_to_screen(bx, by)

            # Follow ring
            pulse = math.sin(t * 0.015) * 3.0
            follow_r = int(r * 1.5 + pulse)
            pygame.draw.circle(self.screen, (255, 255, 255, 140), (bsx, bsy), follow_r, 2)

            # Ball
            pygame.draw.circle(self.screen, (255, 255, 255), (bsx, bsy), int(r * 0.85))
            pygame.draw.circle(self.screen, (20, 20, 30), (bsx, bsy), int(r * 0.55))

    def render_spinner(self, obj: HitObject, t: float, preempt: float, fade_in: float):
        sx, sy = self.osu_to_screen(256, 192)
        total_dur = obj.spinner_end_time - obj.time
        if total_dur <= 0:
            return

        elapsed = t - (obj.time - preempt)
        alpha = min(1.0, max(0.0, elapsed / fade_in))
        if t > obj.spinner_end_time:
            alpha = max(0.0, 1.0 - (t - obj.spinner_end_time) / 200.0)

        int_alpha = int(alpha * 255)
        if int_alpha <= 0:
            return

        spin_r = int(140 * self.scale)

        # Outer ring
        pygame.draw.circle(self.screen, (200, 200, 220), (sx, sy), spin_r, 3)

        # Spinning blades / progress
        if t >= obj.time:
            prog = min(1.0, (t - obj.time) / total_dur)
            # Rotating crosshairs
            angle = (t - obj.time) * 0.01
            for k in range(4):
                a = angle + k * (math.pi / 2.0)
                px = sx + int(math.cos(a) * spin_r * 0.8)
                py = sy + int(math.sin(a) * spin_r * 0.8)
                pygame.draw.line(self.screen, (255, 180, 50), (sx, sy), (px, py), 2)

            # Progress gauge ring
            inner_r = int(spin_r * 0.9)
            rect = pygame.Rect(sx - inner_r, sy - inner_r, inner_r * 2, inner_r * 2)
            pygame.draw.arc(self.screen, (0, 230, 255), rect, 0, prog * 2.0 * math.pi, 5)

            # Spinner Text
            txt = self.font_spinner.render("SPIN!", True, (255, 255, 255))
            self.screen.blit(txt, txt.get_rect(center=(sx, sy - spin_r - 25)))
        else:
            # Preempt warning
            p = elapsed / preempt
            warn_r = int(spin_r * (1.0 + (1.0 - p) * 1.5))
            pygame.draw.circle(self.screen, (255, 120, 50), (sx, sy), warn_r, 2)

    def render_hud(self):
        # Top HUD bar
        hud_h = 70
        hud_rect = pygame.Rect(0, 0, self.width, hud_h)
        pygame.draw.rect(self.screen, (20, 22, 30), hud_rect)
        pygame.draw.line(self.screen, (50, 55, 75), (0, hud_h), (self.width, hud_h), 1)

        # Right side row 1: Speed, Play/Pause status, and Audio indicator
        state_str = "PLAYING" if self.is_playing else "PAUSED"
        state_col = (70, 220, 120) if self.is_playing else (255, 180, 50)
        speed_str = f"Speed: {self.playback_speed:.2f}x  [{state_str}]"
        st_surf = self.font_bold.render(speed_str, True, state_col)
        st_x = self.width - st_surf.get_width() - 16
        self.screen.blit(st_surf, (st_x, 10))

        if self.audio_muted:
            aud_text = "♫ Mute [U]"
            aud_col = (251, 146, 60)
        elif self.music_loaded:
            aud_text = "♫ Music ON"
            aud_col = (74, 222, 128)
        else:
            aud_text = "♫ Clicks"
            aud_col = (148, 163, 184)
        aud_surf = self.font_small.render(aud_text, True, aud_col)
        aud_x = st_x - aud_surf.get_width() - 14
        self.screen.blit(aud_surf, (aud_x, 13))

        # Right side row 2: Engine source badge & action buttons
        commit_tag = f" [{self.rust_commit_hash}]" if self.rust_commit_hash and self.rust_commit_hash != "unknown" else ""
        if self.active_source == "rust":
            badge_text = f"Source: Rust{commit_tag} [S: Switch to Python]"
            badge_col = (30, 130, 60)
        else:
            badge_text = f"Source: Python Standalone [S: Switch to Rust{commit_tag}]"
            badge_col = (25, 95, 160)

        b_surf = self.font_small.render(badge_text, True, (255, 255, 255))
        badge_w = b_surf.get_width() + 16
        badge_h = 24
        badge_x = self.width - badge_w - 16
        badge_y = 36
        pygame.draw.rect(
            self.screen, badge_col, (badge_x, badge_y, badge_w, badge_h), border_radius=4
        )
        self.screen.blit(b_surf, (badge_x + 8, badge_y + 4))

        # Maps Button [M] Карты
        btn_maps_text = "[M] Карты"
        m_surf = self.font_small.render(btn_maps_text, True, (255, 255, 255))
        m_w = m_surf.get_width() + 16
        m_h = 24
        m_x = badge_x - m_w - 10
        m_y = 36
        self.btn_maps_rect = pygame.Rect(m_x, m_y, m_w, m_h)

        mx, my = pygame.mouse.get_pos()
        is_m_hover = self.btn_maps_rect.collidepoint(mx, my)
        m_col = (190, 24, 93) if is_m_hover else (130, 20, 65)
        pygame.draw.rect(self.screen, m_col, self.btn_maps_rect, border_radius=4)
        pygame.draw.rect(self.screen, (244, 114, 182), self.btn_maps_rect, 1, border_radius=4)
        self.screen.blit(m_surf, (m_x + 8, m_y + 4))

        # Tech Info Button [T] Тех. инфо
        btn_tech_text = "[T] Тех. инфо"
        t_surf = self.font_small.render(btn_tech_text, True, (255, 255, 255))
        t_w = t_surf.get_width() + 16
        t_h = 24
        t_x = m_x - t_w - 10
        t_y = 36
        self.btn_tech_rect = pygame.Rect(t_x, t_y, t_w, t_h)

        is_t_hover = self.btn_tech_rect.collidepoint(mx, my)
        t_col = (59, 130, 246) if (self.show_tech_hud or is_t_hover) else (30, 64, 140)
        pygame.draw.rect(self.screen, t_col, self.btn_tech_rect, border_radius=4)
        pygame.draw.rect(self.screen, (96, 165, 250), self.btn_tech_rect, 1, border_radius=4)
        self.screen.blit(t_surf, (t_x + 8, t_y + 4))

        # Skip Intro Button (shown during intro before first note)
        self.btn_skip_rect = None
        row2_left_x = t_x
        if self.beatmap and self.beatmap.hit_objects:
            first_time = self.beatmap.hit_objects[0].time
            preempt, _ = self.beatmap.ar_preempt_fadein
            skip_target = max(0.0, first_time - preempt - 600.0)
            if self.current_time_ms < skip_target - 800.0 and skip_target > 2500.0:
                sk_surf = self.font_small.render("⏭ Пропустить [X]", True, (255, 255, 255))
                sk_w = sk_surf.get_width() + 16
                sk_h = 24
                sk_x = t_x - sk_w - 10
                sk_y = 36
                self.btn_skip_rect = pygame.Rect(sk_x, sk_y, sk_w, sk_h)
                row2_left_x = sk_x
                is_sk_hover = self.btn_skip_rect.collidepoint(mx, my)
                sk_col = (234, 88, 12) if is_sk_hover else (180, 60, 10)
                pygame.draw.rect(self.screen, sk_col, self.btn_skip_rect, border_radius=4)
                pygame.draw.rect(self.screen, (251, 146, 60), self.btn_skip_rect, 1, border_radius=4)
                self.screen.blit(sk_surf, (sk_x + 8, sk_y + 4))

        # Left side: Title & Artist (clipped before audio/speed labels)
        artist_disp = self.beatmap.artist or "Unknown"
        title_disp = self.beatmap.title or "Untitled"
        ver_disp = self.beatmap.version_name or "Normal"
        title_str = f"{artist_disp} - {title_disp} [{ver_disp}]"
        title_surf = self.font_title.render(title_str, True, (240, 240, 245))
        prev_clip = self.screen.get_clip()
        self.screen.set_clip(pygame.Rect(16, 4, max(40, aud_x - 28), 32))
        self.screen.blit(title_surf, (16, 8))

        # Left side row 2: Difficulty & Stats (clipped before action buttons)
        un_tp, act_tp = self.beatmap.get_timing_at(self.current_time_ms)
        bpm_str = f"{un_tp.bpm:.1f}" if un_tp else "120.0"
        sv_str = f"{act_tp.sv_multiplier:.2f}x" if act_tp else "1.00x"

        replay_info = ""
        if self.replay:
            rep_src_tag = (
                "Py fallback"
                if self.active_replay_source == "python_fallback"
                else ("Rust" if self.active_replay_source == "rust" else "Py")
            )
            replay_info = f"  |  Replay ({rep_src_tag}): {self.replay.player} ({self.replay.score:,}) [C]"

        break_cnt = len(getattr(self.beatmap, "breaks", []))
        break_tag = f"  |  Breaks: {break_cnt}" if break_cnt > 0 else ""

        stats_str = (
            f"CS: {self.beatmap.cs:.1f}  AR: {self.beatmap.ar:.1f}  "
            f"OD: {self.beatmap.od:.1f}  HP: {self.beatmap.hp:.1f}  |  "
            f"BPM: {bpm_str}  SV: {sv_str}  |  Objects: {len(self.beatmap.hit_objects)}"
            f"{break_tag}{replay_info}"
        )
        s_surf = self.font_small.render(stats_str, True, (160, 165, 185))
        self.screen.set_clip(pygame.Rect(16, 36, max(40, row2_left_x - 28), 28))
        self.screen.blit(s_surf, (16, 38))
        self.screen.set_clip(prev_clip)

        # Replay Key Overlay if replay is active
        if self.replay and self.show_replay:
            self.render_key_overlay()

    def render_timeline(self):
        timeline_y = self.height - 52
        timeline_h = 24
        pad_x = 20
        bar_w = self.width - pad_x * 2

        # Seekbar background
        bar_rect = pygame.Rect(pad_x, timeline_y, bar_w, timeline_h)
        pygame.draw.rect(self.screen, (28, 30, 42), bar_rect, border_radius=4)
        pygame.draw.rect(self.screen, (60, 65, 85), bar_rect, 1, border_radius=4)

        dur = max(1.0, self.last_time)

        # Progress fill (drawn before break highlights so breaks stay visible across the whole bar)
        prog = max(0.0, min(1.0, self.current_time_ms / dur))
        fill_w = int(bar_w * prog)
        if fill_w > 0:
            fill_rect = pygame.Rect(pad_x, timeline_y, fill_w, timeline_h)
            pygame.draw.rect(self.screen, (50, 120, 220), fill_rect, border_radius=4)

        # Break periods on timeline
        for b_start, b_end in getattr(self.beatmap, "breaks", []):
            if b_end <= 0.0 or b_start >= dur or b_end <= b_start:
                continue
            bx0 = pad_x + int(max(0.0, min(1.0, b_start / dur)) * bar_w)
            bx1 = pad_x + int(max(0.0, min(1.0, b_end / dur)) * bar_w)
            bw = min(pad_x + bar_w - bx0, max(2, bx1 - bx0))
            if bw > 0:
                b_surf = pygame.Surface((bw, timeline_h - 4), pygame.SRCALPHA)
                b_surf.fill((42, 165, 190, 130))
                self.screen.blit(b_surf, (bx0, timeline_y + 2))
                pygame.draw.line(self.screen, (94, 212, 224), (bx0, timeline_y + 2), (bx0 + bw - 1, timeline_y + 2), 1)
                pygame.draw.line(self.screen, (94, 212, 224), (bx0, timeline_y + timeline_h - 3), (bx0 + bw - 1, timeline_y + timeline_h - 3), 1)

        # Object tick markers on timeline
        for obj in self.beatmap.hit_objects:
            tx = pad_x + int((obj.time / dur) * bar_w)
            if pad_x <= tx <= pad_x + bar_w:
                col = (80, 160, 255) if obj.kind == "circle" else (255, 200, 50)
                pygame.draw.line(self.screen, col, (tx, timeline_y + 2), (tx, timeline_y + timeline_h - 3), 1)

        # Scrubber handle
        handle_x = pad_x + fill_w
        pygame.draw.circle(self.screen, (255, 255, 255), (handle_x, timeline_y + timeline_h // 2), 7)
        pygame.draw.circle(self.screen, (50, 120, 220), (handle_x, timeline_y + timeline_h // 2), 4)

        # Time labels & comprehensive controls hint
        cur_min = int(self.current_time_ms // 60000)
        cur_sec = (self.current_time_ms % 60000) / 1000.0
        tot_min = int(self.last_time // 60000)
        tot_sec = (self.last_time % 60000) / 1000.0

        time_str = (
            f"{cur_min:02d}:{cur_sec:06.3f} / {tot_min:02d}:{tot_sec:06.3f}  "
            f"[Space: Pause | X: Пропустить интро | -/+: Оффсет | \u2190/\u2192: Seek | \u2191/\u2193: Speed | T: Инфо | M: Карты | S: Engine | Tab: HUD]"
        )
        t_surf = self.font_small.render(time_str, True, (160, 165, 185))
        self.screen.blit(t_surf, (pad_x, self.height - 22))

    def render_tech_hud(self):
        """Render in-depth technical inspection overlay as a side dock without overlapping playfield."""
        panel_x = self.tech_panel_rect.x
        panel_y = self.tech_panel_rect.y
        panel_w = self.tech_panel_rect.width
        panel_h = self.tech_panel_rect.height

        surf = pygame.Surface((panel_w, panel_h), pygame.SRCALPHA)
        surf.fill((14, 16, 24, 240))
        pygame.draw.rect(surf, (55, 62, 85), (0, 0, panel_w, panel_h), 1, border_radius=6)

        # Header Title
        head_txt = self.font_bold.render("ТЕХНИЧЕСКИЙ ИНСПЕКТОР", True, (244, 114, 182))
        surf.blit(head_txt, (14, 12))

        # Dock Toggle Button: [-> Справа] / [<- Слева]
        dock_btn_text = "[-> Справа]" if self.tech_hud_dock == "left" else "[<- Слева]"
        dock_surf = self.font_small.render(dock_btn_text, True, (160, 180, 220))
        dw = dock_surf.get_width() + 10
        dh = 20
        dx = panel_w - dw - 32
        dy = 10
        self.btn_tech_dock_rect = pygame.Rect(panel_x + dx, panel_y + dy, dw, dh)

        mx, my = pygame.mouse.get_pos()
        is_dock_hover = self.btn_tech_dock_rect.collidepoint(mx, my)
        pygame.draw.rect(surf, (40, 50, 75) if is_dock_hover else (25, 30, 48), (dx, dy, dw, dh), border_radius=3)
        pygame.draw.rect(surf, (80, 95, 140), (dx, dy, dw, dh), 1, border_radius=3)
        surf.blit(dock_surf, (dx + 5, dy + 2))

        # Close Button: X
        close_txt = self.font_bold.render("X", True, (244, 114, 114))
        cw = 20
        ch = 20
        cx = panel_w - 26
        cy = 10
        self.btn_tech_close_rect = pygame.Rect(panel_x + cx, panel_y + cy, cw, ch)
        is_close_hover = self.btn_tech_close_rect.collidepoint(mx, my)
        pygame.draw.rect(surf, (60, 25, 35) if is_close_hover else (35, 20, 28), (cx, cy, cw, ch), border_radius=3)
        pygame.draw.rect(surf, (160, 60, 80), (cx, cy, cw, ch), 1, border_radius=3)
        surf.blit(close_txt, close_txt.get_rect(center=(cx + cw // 2, cy + ch // 2)))

        # 1. Engine, MD5, warnings, and File info
        t = self.current_time_ms
        commit_tag = f" [{self.rust_commit_hash}]" if self.rust_commit_hash and self.rust_commit_hash != "unknown" else ""
        engine_str = f"Rust rosea-core{commit_tag}" if self.active_source == "rust" else "Python Standalone"
        eng_col = (74, 222, 128) if self.active_source == "rust" else (56, 189, 248)

        rv = getattr(self, "replay_verification", None)
        if self.active_replay_source == "rust":
            rep_eng_str = "Rust (normalized)"
            rep_eng_col = (74, 222, 128)
        elif self.active_replay_source == "python_fallback":
            err_tag = rv.error_code if (rv and rv.error_code) else "rejected"
            rep_eng_str = f"Python fallback ({err_tag})"
            rep_eng_col = (251, 146, 60)
        elif self.active_replay_source == "python":
            rep_eng_str = "Python Standalone"
            rep_eng_col = (56, 189, 248)
        else:
            rep_eng_str = "Нет реплея"
            rep_eng_col = (140, 145, 165)

        md5_val = getattr(self.verification, "beatmap_md5", "") or getattr(self.beatmap, "beatmap_md5", "")
        md5_short = md5_val[:12] + "..." if md5_val else "-"
        if rv:
            if rv.hash_status == "ok":
                pair_md5_str = f"{md5_short} (OK)"
                pair_md5_col = (74, 222, 128)
            elif rv.hash_status == "BeatmapHashMismatch":
                pair_md5_str = f"{md5_short} (MISMATCH)"
                pair_md5_col = (248, 113, 113)
            else:
                pair_md5_str = f"{md5_short} ({rv.hash_status})"
                pair_md5_col = (251, 191, 36)
        else:
            pair_md5_str = f"{md5_short} (без .osr)"
            pair_md5_col = (180, 190, 210)

        map_warns = getattr(self.verification, "warnings", []) or getattr(self.beatmap, "rust_warnings", [])
        unk_count = sum(w.get("count", 0) for w in map_warns if w.get("code") == "UnknownBeatmapFields")
        map_warn_str = f"UnknownBeatmapFields ({unk_count})" if unk_count > 0 else "Нет"
        map_warn_col = (251, 191, 36) if unk_count > 0 else (140, 180, 150)

        # 2. Timing calculations
        un_tp, act_tp = self.beatmap.get_timing_at(t)
        bpm = un_tp.bpm if un_tp else 120.0
        beat_len = un_tp.beat_length if un_tp else 500.0
        sv = act_tp.sv_multiplier if act_tp else 1.0
        tp_line = un_tp.line if un_tp else "?"

        # 3. Hit Windows formulas (osu!stable)
        od = self.beatmap.od
        w300 = 80.0 - 6.0 * od
        w100 = 140.0 - 8.0 * od
        w50 = 200.0 - 10.0 * od

        # 4. CS & AR metrics
        cs = self.beatmap.cs
        ar = self.beatmap.ar
        circle_r_osu = 54.4 - 4.48 * cs
        circle_r_screen = circle_r_osu * self.scale
        preempt, fade_in = self.beatmap.ar_preempt_fadein

        # 5. Active / upcoming hit object
        active_obj = None
        for obj in self.beatmap.hit_objects:
            if obj.time - preempt <= t <= obj.end_time + 100.0:
                active_obj = obj
                break
        if not active_obj:
            for obj in self.beatmap.hit_objects:
                if obj.time > t:
                    active_obj = obj
                    break

        # 6. Replay metrics
        cursor_str = "-"
        dist_str = "-"
        dist_col = (220, 225, 240)
        if self.replay:
            pos, keys = self.replay.cursor_at(t)
            if pos:
                cursor_str = f"({pos[0]:.1f}, {pos[1]:.1f}) osu!px"
                if active_obj:
                    dx = pos[0] - active_obj.x
                    dy = pos[1] - active_obj.y
                    dist = math.hypot(dx, dy)
                    in_circle = "[ВНУТРИ]" if dist <= circle_r_osu else "[ВНЕ КРУГА]"
                    dist_col = (74, 222, 128) if dist <= circle_r_osu else (251, 146, 60)
                    dist_str = f"{dist:.1f} osu!px  {in_circle}"

        breaks = getattr(self.beatmap, "breaks", [])
        active_br = self.active_break_at(t)
        if active_br is not None and active_br[1] > active_br[0]:
            rem_br = max(0.0, (active_br[1] - t) / 1000.0)
            br_str = f"ПЕРЕРЫВ ({rem_br:.1f}с ост.) | всего {len(breaks)}"
            br_col = (94, 234, 212)
        else:
            br_str = f"Перерывы: {len(breaks)}"
            if getattr(self.beatmap, "slider_track_override", None) or getattr(self.beatmap, "slider_border", None):
                ov_parts = []
                if getattr(self.beatmap, "slider_track_override", None):
                    ov_parts.append("Track")
                if getattr(self.beatmap, "slider_border", None):
                    ov_parts.append("Border")
                br_str += f" | Colours: {'/'.join(ov_parts)}"
            br_col = (200, 210, 230)

        items = [
            ("ROW", "Движок карты:", engine_str, eng_col),
            ("ROW", "Движок реплея:", rep_eng_str, rep_eng_col),
            ("ROW", "Файл:", os.path.basename(self.beatmap_path), (220, 225, 240)),
            ("ROW", "MD5 пары:", pair_md5_str, pair_md5_col),
            ("ROW", "Предупр. карты:", map_warn_str, map_warn_col),
            ("ROW", "Карта / Цвета:", br_str, br_col),
            ("ROW", "Время карты:", f"{t:08.2f} мс ({t/1000.0:.2f} сек)", (250, 204, 21)),
            ("SECTION", "// ТАЙМИНГ & СКОРОСТЬ"),
            ("ROW", "BPM / BeatLen:", f"{bpm:.1f} BPM ({beat_len:.2f} мс)", (220, 225, 240)),
            ("ROW", "Скорость (SV):", f"{sv:.2f}x (TP строка {tp_line})", (220, 225, 240)),
            ("SECTION", "// СЛОЖНОСТЬ (OD / AR / CS)"),
            ("ROW", "Радиус CS:", f"{circle_r_osu:.2f} osu!px ({circle_r_screen:.1f} px)", (220, 225, 240)),
            ("ROW", "Появление AR:", f"Preempt {preempt:.1f} мс, FadeIn {fade_in:.1f} мс", (220, 225, 240)),
            ("ROW", "Окна OD:", f"300: ±{w300:.1f}мс | 100: ±{w100:.1f}мс | 50: ±{w50:.1f}мс", (167, 139, 250)),
            ("SECTION", "// АКТИВНЫЙ ОБЪЕКТ"),
        ]

        if active_obj:
            obj_kind_str = active_obj.kind.capitalize()
            if active_obj.kind == "slider":
                obj_kind_str = f"Slider ({active_obj.curve_type}, sp={active_obj.spans}, len={active_obj.declared_length:.1f})"
            items.append(("ROW", "Объект:", f"#{active_obj.id} [Строка {active_obj.line}]: {obj_kind_str}", (244, 114, 182)))
            items.append(("ROW", "Время начала:", f"{active_obj.time:.1f} мс (дельта {active_obj.time - t:+.1f} мс)", (220, 225, 240)))
            items.append(("ROW", "Координаты:", f"({active_obj.x:.1f}, {active_obj.y:.1f}) osu!px", (220, 225, 240)))
        else:
            items.append(("ROW", "Объект:", "Нет активных объектов", (140, 145, 165)))

        if self.replay or rv:
            items.append(("SECTION", "// РЕПЛЕЙ ИГРОКА"))
            if self.replay:
                items.append(("ROW", "Курсор:", cursor_str, (220, 225, 240)))
                items.append(("ROW", "Дистанция:", dist_str, dist_col))
                latest_click = self.get_latest_click_at(t)
                if latest_click:
                    items.append(("ROW", "Посл. клик:", latest_click.badge_text, latest_click.color))
            if rv:
                norm = rv.normalization
                if norm is not None:
                    c_t = norm.get("changed_times", 0)
                    c_d = norm.get("initial_dummy_removed", 0)
                    c_b = norm.get("backward_removed", 0)
                    norm_str = f"time:{c_t} dummy:{c_d} back:{c_b}"
                    norm_col = (251, 191, 36) if (c_t + c_d + c_b) > 0 else (140, 180, 150)
                else:
                    norm_str = rv.error_code or "Не нормализован"
                    norm_col = (251, 146, 60)
                items.append(("ROW", "Нормализация:", norm_str, norm_col))
                trailer_w = [w for w in rv.warnings if w.get("code") == "ReplayTrailerNotInterpreted"]
                if trailer_w:
                    tr_sz = trailer_w[0].get("trailer_size", 0)
                    items.append(("ROW", "Предупр. .osr:", f"ReplayTrailerNotInterpreted ({tr_sz} B)", (251, 191, 36)))
        else:
            latest_click = self.get_latest_click_at(t)
            if latest_click:
                items.append(("SECTION", "// ИНСПЕКЦИЯ КЛИКОВ"))
                items.append(("ROW", "Посл. клик:", latest_click.badge_text, latest_click.color))

        items.append(("SECTION", "// АУДИО & СИНХРОНИЗАЦИЯ"))
        track_name = os.path.basename(self.audio_path) if self.audio_path else "Не найден (только клики)"
        track_col = (74, 222, 128) if self.music_loaded else (251, 146, 60)
        items.append(("ROW", "Трек:", track_name, track_col))
        lead_in = getattr(self.beatmap, "audio_lead_in", 0)
        set_id = getattr(self.beatmap, "beatmapset_id", None)
        if set_id is not None and set_id > 0:
            items.append(("ROW", "SetID / LeadIn:", f"{set_id} / {lead_in} мс", (220, 225, 240)))
        items.append(("ROW", "Смещение:", f"{self.audio_offset_ms:+.1f} мс [- / + / Alt+\u2190\u2192]", (220, 225, 240)))
        vol_str = "Выключен [U]" if self.audio_muted else f"Трек {int(self.music_volume*100)}% | Клики {int(self.hitsound_volume*100)}% [U]"
        items.append(("ROW", "Громкость:", vol_str, (167, 139, 250)))

        y_pos = 38
        val_x = 135
        row_step = 16 if panel_h < 600 else 17
        sec_step = 18 if panel_h < 600 else 19
        prev_clip = surf.get_clip()
        for item in items:
            if y_pos + row_step > panel_h - 4:
                break
            if item[0] == "SECTION":
                y_pos += 3
                pygame.draw.line(surf, (45, 52, 75), (14, y_pos + 7), (panel_w - 14, y_pos + 7), 1)
                sec_surf = self.font_small.render(item[1], True, (120, 135, 175))
                surf.blit(sec_surf, (14, y_pos))
                y_pos += sec_step
            else:
                _, label, val, val_col = item
                lbl_surf = self.font_small.render(label, True, (148, 163, 184))
                surf.blit(lbl_surf, (14, y_pos))
                surf.set_clip(pygame.Rect(val_x, y_pos, max(20, panel_w - val_x - 10), row_step + 2))
                val_surf = self.font_small.render(val, True, val_col)
                surf.blit(val_surf, (val_x, y_pos))
                surf.set_clip(prev_clip)
                y_pos += row_step

        self.screen.blit(surf, (panel_x, panel_y))

    def render_verification_overlay(self):
        """Show full comparison report between Rust and Python parsers (map + replay pair)."""
        overlay_w = min(860, self.width - 40)
        overlay_h = min(580, self.height - 80)
        ox = (self.width - overlay_w) // 2
        oy = (self.height - overlay_h) // 2

        surf = pygame.Surface((overlay_w, overlay_h), pygame.SRCALPHA)
        surf.fill((16, 18, 26, 245))
        pygame.draw.rect(surf, (100, 110, 150), (0, 0, overlay_w, overlay_h), 2)

        # Header
        head = self.font_title.render("Rust rosea-core Parser & Pair Diagnostics", True, (255, 255, 255))
        surf.blit(head, (20, 14))

        st = self.verification.status
        col = (70, 220, 120) if st == "PASS" else ((255, 180, 50) if st == "SKIPPED_UNSUPPORTED" else (240, 70, 70))
        st_surf = self.font_bold.render(f"Map Status: {st}", True, col)
        surf.blit(st_surf, (20, 46))

        rv = getattr(self, "replay_verification", None)
        rep_st = rv.status if rv else "NONE"
        rep_col = (
            (70, 220, 120)
            if rep_st == "PASS"
            else ((255, 180, 50) if rep_st in ("SKIPPED_UNSUPPORTED", "NONE") else (240, 70, 70))
        )
        rep_st_surf = self.font_bold.render(f"Replay: {rep_st}", True, rep_col)
        surf.blit(rep_st_surf, (210, 46))

        map_src = "Rust" if self.active_source == "rust" else "Python"
        rep_src = (
            "Rust"
            if self.active_replay_source == "rust"
            else ("Python (fallback)" if self.active_replay_source == "python_fallback" else ("Python" if self.active_replay_source == "python" else "None"))
        )
        active_str = f"Active Data: Map={map_src} | Replay={rep_src}"
        act_surf = self.font_small.render(active_str, True, (100, 220, 255))
        surf.blit(act_surf, (410, 49))

        md5_val = getattr(self.verification, "beatmap_md5", "") or getattr(self.beatmap, "beatmap_md5", "")
        map_warns = getattr(self.verification, "warnings", [])
        if map_warns:
            map_warn_str = ", ".join(
                f"{w.get('code')}(count={w.get('count')})"
                if "count" in w
                else str(w.get("code"))
                for w in map_warns
            )
        else:
            map_warn_str = "None"

        if rv:
            pair_md5_str = (
                "OK (matches replay header)"
                if rv.hash_status == "ok"
                else (
                    "BeatmapHashMismatch"
                    if rv.hash_status == "BeatmapHashMismatch"
                    else rv.hash_status
                )
            )
        else:
            pair_md5_str = "No replay paired"

        sto = getattr(self.beatmap, "slider_track_override", None)
        sb = getattr(self.beatmap, "slider_border", None)
        lines = [
            (f"Beatmap: {os.path.basename(self.beatmap_path)}  |  MD5: {md5_val or '-'}", (220, 225, 240)),
            (
                f"Objects: {len(self.py_beatmap.hit_objects)}  |  TPs: {len(self.py_beatmap.timing_points)}  |  "
                f"Breaks: {len(getattr(self.py_beatmap, 'breaks', []))}  |  StackLeniency: {self.py_beatmap.stack_leniency:.2f}",
                (210, 215, 230),
            ),
            (
                f"Rust Supported Map: {'Yes (v14 Standard: Circles, L/B/P-Sliders, Spinners)' if self.verification.rust_supported else 'No (Unsupported objects, curve, or syntax)'}",
                (190, 225, 200) if self.verification.rust_supported else (255, 200, 140),
            ),
            (f"Colours Overrides: Track={sto or 'default'}, Border={sb or 'default'}", (200, 205, 220)),
            (f"Map Warnings: {map_warn_str}", (251, 191, 36) if map_warns else (160, 175, 195)),
            (f"Map Summary: {self.verification.reason}", (210, 215, 230)),
            (f"Pair MD5 Check: {pair_md5_str}", (74, 222, 128) if (rv and rv.hash_status == "ok") else ((248, 113, 113) if (rv and rv.hash_status == "BeatmapHashMismatch") else (200, 205, 220))),
        ]

        if rv:
            r_data = rv.rust_replay_data or {}
            hdr = r_data.get("header") or {}
            exp = r_data.get("expected") or {}
            py_r = rv.py_replay
            player = hdr.get("player", py_r.player if py_r else "?")
            g_ver = hdr.get("game_version", py_r.game_version if py_r else "?")
            mods = hdr.get("mods", py_r.mods if py_r else "?")
            seed = hdr.get("seed", py_r.seed if py_r else None)
            tr_sz = hdr.get("trailer_size", py_r.trailer_size if py_r else 0)
            score = exp.get("score", py_r.score if py_r else 0)
            m_combo = exp.get("max_combo", py_r.max_combo if py_r else 0)
            c300 = exp.get("count_300", py_r.count_300 if py_r else 0)
            c100 = exp.get("count_100", py_r.count_100 if py_r else 0)
            c50 = exp.get("count_50", py_r.count_50 if py_r else 0)
            cmiss = exp.get("count_miss", py_r.count_miss if py_r else 0)

            lines.append((f"Replay Summary: {rv.reason}", (210, 215, 230)))
            lines.append((f"Replay Header: player={player}, game_version={g_ver}, mods={mods}, seed={seed}, trailer={tr_sz} B", (200, 210, 230)))
            lines.append((f"Expected Stats: score={score:,}, max_combo={m_combo}x, 300/100/50/M={c300}/{c100}/{c50}/{cmiss}", (200, 210, 230)))

            rep_warns = []
            for w in rv.warnings:
                code = w.get("code")
                if code == "ReplayTrailerNotInterpreted":
                    rep_warns.append(f"ReplayTrailerNotInterpreted({w.get('trailer_size', tr_sz)} B)")
                elif code:
                    rep_warns.append(str(code))
            lines.append((f"Replay Warnings: {', '.join(rep_warns) if rep_warns else 'None'}", (251, 191, 36) if rep_warns else (160, 175, 195)))

            norm = rv.normalization
            if norm is not None:
                c_t = norm.get("changed_times", 0)
                c_d = norm.get("initial_dummy_removed", 0)
                c_b = norm.get("backward_removed", 0)
                sample_ch = norm.get("changes", [])[:3]
                ch_desc = ""
                if sample_ch:
                    parts = []
                    for ch in sample_ch:
                        k = ch.get("kind")
                        if k == "Time":
                            parts.append(f"#{ch.get('index')}: {ch.get('old_ms')}->{ch.get('new_ms')}ms")
                        elif k == "InitialDummy":
                            parts.append(f"#{ch.get('index')}:Dummy")
                        elif k == "BackwardFrame":
                            parts.append(f"#{ch.get('index')}:Backward")
                    more = f" (+{len(norm.get('changes', [])) - len(sample_ch)} more)" if len(norm.get("changes", [])) > len(sample_ch) else ""
                    ch_desc = f" [{', '.join(parts)}{more}]"
                lines.append((f"Normalization: changed_times={c_t}, initial_dummy_removed={c_d}, backward_removed={c_b}{ch_desc}", (167, 243, 208)))

        all_mismatches = list(self.verification.mismatches)
        if rv and rv.mismatches:
            all_mismatches.extend(rv.mismatches)
        if all_mismatches:
            lines.append(("Discrepancies:", (248, 113, 113)))
            for m in all_mismatches[:5]:
                lines.append((f"  * {m}", (248, 160, 160)))

        line_step = 18 if overlay_h < 460 else 21
        y_off = 72 if overlay_h < 460 else 76
        prev_clip = surf.get_clip()
        surf.set_clip(pygame.Rect(16, 70, overlay_w - 32, overlay_h - 104))
        for line_txt, line_col in lines:
            if y_off + line_step - 2 > overlay_h - 34:
                break
            txt = self.font_small.render(line_txt, True, line_col)
            surf.blit(txt, (20, y_off))
            y_off += line_step
        surf.set_clip(prev_clip)

        close_hint = self.font_small.render("Press 'V' or 'Tab' to close this overlay  |  Press 'S' to toggle Engine", True, (140, 145, 165))
        surf.blit(close_hint, (20, overlay_h - 28))

        self.screen.blit(surf, (ox, oy))

    def handle_mouse_seek(self, mouse_x: int):
        pad_x = 20
        bar_w = self.width - pad_x * 2
        frac = max(0.0, min(1.0, (mouse_x - pad_x) / bar_w))
        self.seek_to(frac * self.last_time)

    def run(self, max_frames: Optional[int] = None):
        running = True
        frame_count = 0
        last_ticks = pygame.time.get_ticks()

        while running:
            # Check for map selection from non-blocking companion window
            if self.map_browser:
                new_map = self.map_browser.get_selected_map()
                if new_map:
                    if isinstance(new_map, tuple):
                        m_path, r_path = new_map
                        self.load_beatmap(m_path, replay_path=r_path)
                    else:
                        self.load_beatmap(new_map)

            current_ticks = pygame.time.get_ticks()
            dt = current_ticks - last_ticks
            last_ticks = current_ticks
            dt = min(100, max(1, dt))

            for event in pygame.event.get():
                if event.type == pygame.QUIT:
                    running = False

                elif event.type == pygame.VIDEORESIZE:
                    self.width = max(640, event.w)
                    self.height = max(480, event.h)
                    self.screen = pygame.display.set_mode(
                        (self.width, self.height), pygame.RESIZABLE
                    )
                    self.update_playfield_layout()
                    self.build_slider_caches()

                elif event.type == pygame.KEYDOWN:
                    mods = pygame.key.get_mods()
                    is_alt = bool(mods & pygame.KMOD_ALT)
                    is_shift = bool(mods & pygame.KMOD_SHIFT)
                    is_ctrl = bool(mods & pygame.KMOD_CTRL)

                    if event.key == pygame.K_ESCAPE:
                        running = False
                    elif event.key == pygame.K_SPACE:
                        self.toggle_playback()
                    elif event.key == pygame.K_u:
                        self.toggle_audio_mute()
                    elif event.key == pygame.K_m:
                        self.open_map_browser()
                    elif event.key in (pygame.K_PAGEUP, pygame.K_LEFTBRACKET):
                        self.switch_relative_map(-1)
                    elif event.key in (pygame.K_PAGEDOWN, pygame.K_RIGHTBRACKET):
                        self.switch_relative_map(1)
                    elif event.key == pygame.K_LEFT:
                        if is_alt:
                            self.adjust_audio_offset(-5.0)
                        else:
                            step = 5000.0 if is_shift else 2000.0
                            self.seek_to(self.current_time_ms - step)
                    elif event.key == pygame.K_RIGHT:
                        if is_alt:
                            self.adjust_audio_offset(5.0)
                        else:
                            step = 5000.0 if is_shift else 2000.0
                            self.seek_to(self.current_time_ms + step)
                    elif event.key == pygame.K_UP:
                        delta = 0.25 if is_ctrl else (0.05 if is_shift else 0.01)
                        self.change_speed(delta)
                        self.speed_key_held = pygame.K_UP
                        now = time.time()
                        self.speed_key_press_time = now
                        self.speed_key_last_repeat = now
                    elif event.key == pygame.K_DOWN:
                        delta = 0.25 if is_ctrl else (0.05 if is_shift else 0.01)
                        self.change_speed(-delta)
                        self.speed_key_held = pygame.K_DOWN
                        now = time.time()
                        self.speed_key_press_time = now
                        self.speed_key_last_repeat = now
                    elif event.key in (pygame.K_0, pygame.K_KP0):
                        self.set_speed(1.0)
                    elif event.key in (pygame.K_r, pygame.K_HOME):
                        self.seek_to(self.start_time)
                    elif event.key in (pygame.K_z, pygame.K_d):
                        self.record_live_click("K1")
                    elif event.key in (pygame.K_x, pygame.K_f):
                        if self.can_skip_intro() and event.key == pygame.K_x:
                            self.skip_intro()
                        else:
                            self.record_live_click("K2")
                    elif event.key in (pygame.K_MINUS, pygame.K_KP_MINUS):
                        self.adjust_audio_offset(-5.0)
                    elif event.key in (pygame.K_EQUALS, pygame.K_PLUS, pygame.K_KP_PLUS):
                        self.adjust_audio_offset(5.0)
                    elif event.key == pygame.K_s:
                        self.toggle_engine_source()
                    elif event.key == pygame.K_c:
                        if self.replay:
                            self.show_replay = not self.show_replay
                            self.set_toast(
                                f"Replay cursor {'enabled' if self.show_replay else 'disabled'}",
                                1.5,
                            )
                        else:
                            self.set_toast("No .osr replay available for this map", 2.0)
                    elif event.key == pygame.K_v:
                        self.show_verification_detail = not self.show_verification_detail
                    elif event.key == pygame.K_t:
                        self.toggle_tech_hud()
                    elif event.key in (pygame.K_TAB, pygame.K_F1):
                        self.show_hud = not self.show_hud

                elif event.type == pygame.MOUSEBUTTONDOWN:
                    mx, my = event.pos
                    if event.button == 1:
                        if self.btn_skip_rect and self.btn_skip_rect.collidepoint(mx, my):
                            self.skip_intro()
                        elif self.btn_maps_rect.collidepoint(mx, my):
                            self.open_map_browser()
                        elif self.btn_tech_rect.collidepoint(mx, my):
                            self.toggle_tech_hud()
                        elif self.show_tech_hud and self.btn_tech_close_rect.collidepoint(mx, my):
                            self.toggle_tech_hud()
                        elif self.show_tech_hud and self.btn_tech_dock_rect.collidepoint(mx, my):
                            self.toggle_tech_hud_dock()
                        elif my >= self.height - 60:
                            self.is_scrubbing = True
                            self.handle_mouse_seek(mx)
                        elif self.pf_rect.collidepoint(mx, my):
                            self.record_live_click("M1")
                    elif event.button == 3:
                        if self.pf_rect.collidepoint(mx, my):
                            self.record_live_click("M2")

                elif event.type == pygame.KEYUP:
                    if event.key in (pygame.K_UP, pygame.K_DOWN) and self.speed_key_held == event.key:
                        self.speed_key_held = None

                elif event.type == pygame.MOUSEBUTTONUP:
                    if event.button == 1:
                        self.is_scrubbing = False

                elif event.type == pygame.MOUSEMOTION:
                    if self.is_scrubbing:
                        self.handle_mouse_seek(event.pos[0])

            # Smooth speed adjustment while holding UP or DOWN arrow key
            if self.speed_key_held in (pygame.K_UP, pygame.K_DOWN):
                pressed = pygame.key.get_pressed()
                if not pressed[self.speed_key_held]:
                    self.speed_key_held = None
                else:
                    now = time.time()
                    if now - self.speed_key_press_time >= 0.22 and now - self.speed_key_last_repeat >= 0.035:
                        mods = pygame.key.get_mods()
                        is_shift = bool(mods & pygame.KMOD_SHIFT)
                        is_ctrl = bool(mods & pygame.KMOD_CTRL)
                        delta = 0.25 if is_ctrl else (0.05 if is_shift else 0.01)
                        if self.speed_key_held == pygame.K_UP:
                            self.change_speed(delta)
                        else:
                            self.change_speed(-delta)
                        self.speed_key_last_repeat = now

            self.step(dt)
            self.render()
            pygame.display.flip()

            frame_count += 1
            if max_frames and frame_count >= max_frames:
                break

            self.clock.tick(60)

        if self.map_browser:
            self.map_browser.close()
        pygame.quit()


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Rosea osu! Beatmap Visualizer & Tester")
    parser.add_argument("beatmap", nargs="?", default="tests/data/01_micro/2dearbrave.osu", help="Path to .osu beatmap")
    parser.add_argument("--replay", type=str, default=None, help="Path to .osr replay file to visualize alongside beatmap")
    parser.add_argument("--source", choices=["auto", "rust", "python"], default="auto", help="Data source engine (auto, rust, python)")
    parser.add_argument("--audio", type=str, default=None, help="Explicit path to background audio file (.mp3 / .ogg)")
    parser.add_argument("--no-audio", action="store_true", help="Disable music audio playback")
    parser.add_argument("--no-hitsounds", action="store_true", help="Disable hitsounds (note click sounds)")
    parser.add_argument("--offset", type=float, default=0.0, help="Universal audio offset in milliseconds (default: 0.0)")
    parser.add_argument("--osu-dir", type=str, default=None, help="Path to osu! stable installation folder (default: autodetect %%LOCALAPPDATA%%/osu!)")
    parser.add_argument("--test", action="store_true", help="Run automated test suite verifying Rust parser across corpus")
    parser.add_argument("--verify", action="store_true", help="Verify single map against Rust parser and exit")
    parser.add_argument("--headless", action="store_true", help="Run visualizer in headless dummy mode for testing")
    parser.add_argument("--frames", type=int, default=None, help="Exit after N frames (useful for automated testing)")
    parser.add_argument("--width", type=int, default=1024, help="Window width")
    parser.add_argument("--height", type=int, default=768, help="Window height")

    args = parser.parse_args()

    if args.test:
        print("=== Running Rosea Rust Parser Verification Test Suite ===")
        report = run_full_corpus_verification()
        print(f"Total Maps Tested: {report['total']}")
        print(f"Fully Verified with Rust: {len(report['passed'])}")
        for r in report['passed']:
            print(f"  [PASS] {os.path.basename(r.file_path)}: {r.reason}")
        print(f"\nSkipped (Gracefully rejected unsupported curves/spinners): {len(report['skipped'])}")
        for r in report['skipped']:
            print(f"  [SKIP] {os.path.basename(r.file_path)}: {r.reason}")
        print(f"\nFailed: {len(report['failed'])}")
        for r in report['failed']:
            print(f"  [FAIL] {os.path.basename(r.file_path)}: {r.reason}")
        if report['failed']:
            sys.exit(1)
        print("\nAll tests passed successfully!")
        return

    if args.verify:
        res = verify_beatmap(args.beatmap, replay_path=args.replay)
        print(f"Verification Report for {args.beatmap}:")
        print(f"  Status: {res.status}")
        print(f"  MD5:    {res.beatmap_md5}")
        print(f"  Reason: {res.reason}")
        if res.warnings:
            print(f"  Warnings: {res.warnings}")
        if res.replay_verification:
            rv = res.replay_verification
            print(f"  Replay: {rv.status} ({rv.reason})")
            if rv.warnings:
                print(f"  Replay Warnings: {rv.warnings}")
            if rv.normalization is not None:
                print(f"  Normalization: {rv.normalization}")
            if rv.mismatches:
                print("  Replay Discrepancies:")
                for m in rv.mismatches:
                    print(f"    - {m}")
        if res.mismatches:
            print("  Discrepancies:")
            for m in res.mismatches:
                print(f"    - {m}")
        if res.status == "FAIL" or (
            res.replay_verification
            and res.replay_verification.status in ("FAIL", "HASH_MISMATCH")
        ):
            sys.exit(1)
        return

    # Visualizer mode
    beatmap_path = args.beatmap
    replay_path = args.replay

    # If the user passed a .osr replay as the first positional argument,
    # automatically look for a matching .osu beatmap next to it!
    if beatmap_path.lower().endswith(".osr") and replay_path is None:
        candidate_osu = os.path.splitext(beatmap_path)[0] + ".osu"
        if os.path.isfile(candidate_osu):
            replay_path = beatmap_path
            beatmap_path = candidate_osu
            print(f"Notice: Detected .osr file, automatically paired with '{beatmap_path}'")

    if not os.path.isfile(beatmap_path):
        print(f"Error: Beatmap file '{beatmap_path}' not found.", file=sys.stderr)
        # Search for possible matches to help the user
        filename = os.path.basename(beatmap_path)
        matches = []
        for root, _, files in os.walk("tests/data"):
            if filename in files:
                matches.append(os.path.join(root, filename).replace("\\", "/"))
        if matches:
            print(f"Did you mean one of these?", file=sys.stderr)
            for m in matches:
                print(f"  python scripts/visualizer.py {m}", file=sys.stderr)
        sys.exit(1)

    vis = Visualizer(
        beatmap_path,
        replay_path=replay_path,
        source=args.source,
        width=args.width,
        height=args.height,
        headless=args.headless,
        audio_path=args.audio,
        disable_audio=args.no_audio,
        disable_hitsounds=args.no_hitsounds,
        audio_offset=args.offset,
        osu_dir=args.osu_dir,
    )
    vis.run(max_frames=args.frames)


if __name__ == "__main__":
    main()
