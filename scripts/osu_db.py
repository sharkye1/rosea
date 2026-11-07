import os
import struct
import time
from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass


@dataclass
class ReplayInfo:
    bm_hash: str
    player: str
    score: int
    combo: int
    c300: int
    c100: int
    c50: int
    miss: int
    mods: int
    timestamp: int
    file_path: str
    source_folder: str  # 'Replays' or 'Data/r'


def parse_replay_header_fast(path: str, source_folder: str = "") -> Optional[ReplayInfo]:
    """Extract metadata from .osr file without decompressing frames (<0.1ms)."""
    try:
        with open(path, "rb") as f:
            data = f.read(4096)
        if len(data) < 35:
            return None
        mode = data[0]
        if mode != 0:  # osu!standard only
            return None

        offset = 5

        def read_str() -> str:
            nonlocal offset
            if offset >= len(data):
                return ""
            b = data[offset]
            offset += 1
            if b != 0x0B:
                return ""
            length, shift = 0, 0
            while True:
                by = data[offset]
                offset += 1
                length |= (by & 0x7F) << shift
                if not (by & 0x80):
                    break
                shift += 7
            val = data[offset : offset + length].decode("utf-8", errors="ignore")
            offset += length
            return val

        bm_hash = read_str()
        player = read_str()
        rp_hash = read_str()
        c300, c100, c50, cgeki, ckatu, cmiss = struct.unpack(
            "<HHHHHH", data[offset : offset + 12]
        )
        offset += 12
        score, combo = struct.unpack("<IH", data[offset : offset + 6])
        offset += 6
        perfect = data[offset]
        offset += 1
        mods = struct.unpack("<I", data[offset : offset + 4])[0]
        offset += 4
        life = read_str()
        timestamp = struct.unpack("<q", data[offset : offset + 8])[0]

        return ReplayInfo(
            bm_hash=bm_hash,
            player=player if player else "Player",
            score=score,
            combo=combo,
            c300=c300,
            c100=c100,
            c50=c50,
            miss=cmiss,
            mods=mods,
            timestamp=timestamp,
            file_path=path,
            source_folder=source_folder,
        )
    except Exception:
        return None


class OsuStableDatabase:
    """Fast, safe parser and indexer for osu! stable (osu!.db, Replays/, Data/r/)."""

    def __init__(self, osu_dir: Optional[str] = None):
        if not osu_dir:
            osu_dir = os.path.join(os.path.expanduser("~"), "AppData", "Local", "osu!")
        self.osu_dir = os.path.normpath(osu_dir)
        self.osu_db_path = os.path.join(self.osu_dir, "osu!.db")
        self.songs_dir = os.path.join(self.osu_dir, "Songs")
        self.replays_dir = os.path.join(self.osu_dir, "Replays")
        self.data_r_dir = os.path.join(self.osu_dir, "Data", "r")

        self.beatmap_by_md5: Dict[str, dict] = {}
        self.last_parsed_time: float = 0.0

    def is_available(self) -> bool:
        return os.path.isdir(self.osu_dir) and os.path.isfile(self.osu_db_path)

    def load_beatmaps(self, force: bool = False) -> Dict[str, dict]:
        """Parse osu!.db in binary into a dict indexed by MD5 (<80ms for 5500 maps)."""
        if not self.is_available():
            return {}

        mtime = os.path.getmtime(self.osu_db_path)
        if self.beatmap_by_md5 and mtime <= self.last_parsed_time and not force:
            return self.beatmap_by_md5

        try:
            with open(self.osu_db_path, "rb") as f:
                data = f.read()

            offset = 0
            ver = struct.unpack_from("<i", data, offset)[0]
            offset += 4
            folder_count = struct.unpack_from("<i", data, offset)[0]
            offset += 4
            unlocked = struct.unpack_from("<?", data, offset)[0]
            offset += 1
            unlock_date = struct.unpack_from("<q", data, offset)[0]
            offset += 8

            def read_str() -> str:
                nonlocal offset
                if offset >= len(data):
                    return ""
                b = data[offset]
                offset += 1
                if b == 0:
                    return ""
                if b != 0x0B:
                    return ""
                length, shift = 0, 0
                while True:
                    by = data[offset]
                    offset += 1
                    length |= (by & 0x7F) << shift
                    if not (by & 0x80):
                        break
                    shift += 7
                val = data[offset : offset + length].decode("utf-8", errors="ignore")
                offset += length
                return val

            user_name = read_str()
            bm_count = struct.unpack_from("<i", data, offset)[0]
            offset += 4

            def skip_star_ratings():
                nonlocal offset
                cnt = struct.unpack_from("<i", data, offset)[0]
                offset += 4
                for _ in range(cnt):
                    offset += 5  # 0x08 + mod(4)
                    b2 = data[offset]
                    offset += 1
                    if b2 == 0x0C:
                        offset += 4
                    else:
                        offset += 8

            maps: Dict[str, dict] = {}
            for _ in range(bm_count):
                artist = read_str()
                artist_u = read_str()
                title = read_str()
                title_u = read_str()
                creator = read_str()
                version = read_str()
                audio = read_str()
                md5 = read_str()
                osu_file = read_str()

                ranked = data[offset]
                offset += 1
                circles, sliders, spinners = struct.unpack_from("<HHH", data, offset)
                offset += 6
                last_mod = struct.unpack_from("<q", data, offset)[0]
                offset += 8
                ar, cs, hp, od = struct.unpack_from("<ffff", data, offset)
                offset += 16
                sv = struct.unpack_from("<d", data, offset)[0]
                offset += 8

                for _ in range(4):
                    skip_star_ratings()

                drain_sec, total_ms, preview_ms = struct.unpack_from("<iii", data, offset)
                offset += 12
                tp_count = struct.unpack_from("<i", data, offset)[0]
                offset += 4
                offset += tp_count * 17

                bm_id, set_id, thread_id = struct.unpack_from("<iii", data, offset)
                offset += 12
                offset += 10  # grades(4), offset(2), stack_len(4)
                mode = data[offset]
                offset += 1
                src = read_str()
                tags = read_str()
                offset += 2  # online_offset
                font = read_str()
                offset += 10  # unplayed, last_played, is_osz2
                folder = read_str()
                offset += 18  # last_checked, flags, mod_cache, mania_speed

                if mode == 0 and md5:  # osu!standard
                    maps[md5] = {
                        "artist": artist,
                        "title": title,
                        "version": version,
                        "creator": creator,
                        "folder": folder,
                        "osu_file": osu_file,
                        "set_id": set_id,
                        "bm_id": bm_id,
                        "audio": audio,
                        "cs": cs,
                        "ar": ar,
                        "od": od,
                        "hp": hp,
                        "circles": circles,
                        "sliders": sliders,
                        "spinners": spinners,
                        "objects": circles + sliders + spinners,
                        "full_path": os.path.join(self.songs_dir, folder, osu_file),
                    }

            self.beatmap_by_md5 = maps
            self.last_parsed_time = mtime
            return self.beatmap_by_md5
        except Exception as e:
            print(f"Warning: error parsing osu!.db: {e}", file=sys.stderr)
            return self.beatmap_by_md5

    def scan_replays_and_match(self) -> List[Tuple[dict, ReplayInfo]]:
        """Find all available .osr files in osu! and match them with beatmaps."""
        bm_map = self.load_beatmaps()
        if not bm_map:
            return []

        matched_pairs: List[Tuple[dict, ReplayInfo]] = []
        seen_keys = set()

        # 1. Check Replays/ folder (exported replays)
        if os.path.isdir(self.replays_dir):
            try:
                for f in sorted(os.listdir(self.replays_dir)):
                    if f.lower().endswith(".osr"):
                        p = os.path.join(self.replays_dir, f)
                        info = parse_replay_header_fast(p, source_folder="Replays")
                        if info and info.bm_hash in bm_map:
                            bm = bm_map[info.bm_hash]
                            if os.path.isfile(bm["full_path"]):
                                key = (bm["full_path"], p)
                                if key not in seen_keys:
                                    seen_keys.add(key)
                                    matched_pairs.append((bm, info))
            except Exception:
                pass

        # 2. Check Data/r/ folder (local scores & saved replays)
        if os.path.isdir(self.data_r_dir):
            try:
                # In Data/r, files are named <bm_hash>-<timestamp>.osr
                for f in sorted(os.listdir(self.data_r_dir), reverse=True):
                    if f.lower().endswith(".osr"):
                        prefix_hash = f.split("-")[0].lower()
                        if prefix_hash in bm_map:
                            bm = bm_map[prefix_hash]
                            if os.path.isfile(bm["full_path"]):
                                p = os.path.join(self.data_r_dir, f)
                                info = parse_replay_header_fast(p, source_folder="Data/r")
                                if info:
                                    key = (bm["full_path"], p)
                                    if key not in seen_keys:
                                        seen_keys.add(key)
                                        matched_pairs.append((bm, info))
            except Exception:
                pass

        return matched_pairs
