#!/usr/bin/env python3
"""
Rosea Map Browser - Non-blocking companion window for selecting beatmaps.
Scans tests/data/ and automatically detects osu! stable (osu!.db, Replays/, Data/r/)
for all available beatmap + replay pairs. Displays metadata, difficulty stats,
player info, high scores, Rust compatibility, and replay availability with real-time search.
"""

import glob
import os
import queue
import sys
import threading
import tkinter as tk
from dataclasses import dataclass
from tkinter import ttk
from typing import Any, Callable, Dict, List, Optional, Tuple

try:
    from osu_db import OsuStableDatabase, parse_replay_header_fast
except ImportError:
    from scripts.osu_db import OsuStableDatabase, parse_replay_header_fast


@dataclass
class MapEntry:
    file_path: str
    category: str
    filename: str
    title: str
    artist: str
    version: str
    cs: float
    ar: float
    od: float
    hp: float
    objects_count: int
    has_replay: bool
    rust_supported: bool
    display_title: str
    replay_path: Optional[str] = None
    player_name: str = ""
    replay_score: int = 0
    replay_combo: int = 0
    replay_mods: int = 0
    source_type: str = "tests"  # "tests" or "stable"

    def matches(self, query: str) -> bool:
        q = query.lower().strip()
        if not q:
            return True
        return (
            q in self.title.lower()
            or q in self.artist.lower()
            or q in self.version.lower()
            or q in self.filename.lower()
            or q in self.category.lower()
            or q in self.file_path.lower()
            or q in self.player_name.lower()
            or (self.replay_path and q in self.replay_path.lower())
        )


def fast_scan_beatmap(path: str) -> Optional[MapEntry]:
    """Quickly extract metadata and difficulty from .osu file without full simulation."""
    try:
        norm_path = path.replace("\\", "/")
        parts = norm_path.split("/")
        category = parts[-2] if len(parts) >= 2 else "other"
        filename = os.path.basename(path)

        # Check for adjacent or folder-local .osr replay
        candidate_osr = os.path.splitext(path)[0] + ".osr"
        has_replay = False
        replay_path = None
        player_name = ""
        replay_score = 0
        replay_combo = 0

        folder = os.path.dirname(path)
        if os.path.isfile(candidate_osr):
            replay_path = candidate_osr
        elif os.path.isdir(folder):
            for f in os.listdir(folder):
                if f.lower().endswith(".osr"):
                    cand = os.path.join(folder, f)
                    if os.path.isfile(cand):
                        replay_path = cand
                        break

        if replay_path and os.path.isfile(replay_path):
            has_replay = True
            r_info = parse_replay_header_fast(replay_path)
            if r_info:
                player_name = r_info.player
                replay_score = r_info.score
                replay_combo = r_info.combo

        title = "Unknown"
        artist = "Unknown"
        version = "Normal"
        cs = 4.0
        od = 8.0
        hp = 5.0
        ar = None
        has_sliders = False
        has_complex_objects = False
        obj_count = 0

        with open(path, "r", encoding="utf-8", errors="replace") as f:
            section = None
            for raw_line in f:
                line = raw_line.strip()
                if not line or line.startswith("//"):
                    continue
                if line.startswith("[") and line.endswith("]"):
                    section = line
                    continue

                if section == "[Metadata]":
                    if ":" in line:
                        k, v = line.split(":", 1)
                        k = k.strip()
                        v = v.strip()
                        if k == "Title":
                            title = v
                        elif k == "Artist":
                            artist = v
                        elif k == "Version":
                            version = v
                elif section == "[Difficulty]":
                    if ":" in line:
                        k, v = line.split(":", 1)
                        k = k.strip()
                        v = v.strip()
                        try:
                            val = float(v)
                            if k == "CircleSize":
                                cs = val
                            elif k == "OverallDifficulty":
                                od = val
                            elif k == "HPDrainRate":
                                hp = val
                            elif k == "ApproachRate":
                                ar = val
                        except ValueError:
                            pass
                elif section == "[HitObjects]":
                    obj_count += 1
                    fields = line.split(",")
                    if len(fields) >= 4:
                        try:
                            flags = int(fields[3].strip())
                            base_type = flags & (1 | 2 | 8 | 128)
                            if base_type == 2:
                                has_sliders = True
                                if len(fields) >= 6:
                                    tail = fields[5].strip()
                                    if tail and not tail.startswith("L|") and not tail.startswith("L,"):
                                        has_complex_objects = True
                            elif base_type in (8, 128) or base_type not in (1, 2):
                                has_complex_objects = True
                        except ValueError:
                            pass

        if ar is None:
            ar = od

        rust_supported = not has_complex_objects and obj_count > 0
        display = f"{artist} - {title} [{version}]"

        return MapEntry(
            file_path=norm_path,
            category=category,
            filename=filename,
            title=title,
            artist=artist,
            version=version,
            cs=cs,
            ar=ar,
            od=od,
            hp=hp,
            objects_count=obj_count,
            has_replay=has_replay,
            rust_supported=rust_supported,
            display_title=display,
            replay_path=replay_path,
            player_name=player_name,
            replay_score=replay_score,
            replay_combo=replay_combo,
            source_type="tests",
        )
    except Exception:
        return None


def scan_all_maps(
    root_dir: str = "tests/data",
    osu_dir: Optional[str] = None,
) -> List[MapEntry]:
    """
    Safely and quickly scan available beatmaps:
    1. Local tests/data directory.
    2. osu! stable installation (osu!.db + Replays/ + Data/r/) matching pairs (<100ms).
    """
    entries: List[MapEntry] = []
    seen_paths = set()

    # 1. Scan tests/data directory
    if os.path.isdir(root_dir):
        pattern = os.path.join(root_dir, "**", "*.osu")
        files = sorted(glob.glob(pattern, recursive=True))
        for f in files:
            e = fast_scan_beatmap(f)
            if e and e.file_path not in seen_paths:
                seen_paths.add(e.file_path)
                entries.append(e)

    # 2. Automatically scan osu! stable for matched map + replay pairs
    db = OsuStableDatabase(osu_dir)
    if db.is_available():
        try:
            stable_pairs = db.scan_replays_and_match()
            for bm, rep in stable_pairs:
                osu_path = bm["full_path"]
                key = (osu_path, rep.file_path)
                if key in seen_paths:
                    continue
                seen_paths.add(key)

                category = f"osu! {rep.source_folder}"
                display = f"{bm['artist']} - {bm['title']} [{bm['version']}]"
                rust_ok = (bm["sliders"] == 0 and bm["spinners"] == 0)

                entry = MapEntry(
                    file_path=osu_path,
                    category=category,
                    filename=bm["osu_file"],
                    title=bm["title"],
                    artist=bm["artist"],
                    version=bm["version"],
                    cs=bm["cs"],
                    ar=bm["ar"],
                    od=bm["od"],
                    hp=bm["hp"],
                    objects_count=bm["objects"],
                    has_replay=True,
                    rust_supported=rust_ok,
                    display_title=display,
                    replay_path=rep.file_path,
                    player_name=rep.player,
                    replay_score=rep.score,
                    replay_combo=rep.combo,
                    source_type="stable",
                )
                entries.append(entry)
        except Exception as e:
            print(f"Warning: error scanning osu! stable pairs: {e}", file=sys.stderr)

    return entries


class MapBrowserApp:
    """Tkinter-based companion browser window running on a dedicated thread."""

    def __init__(
        self,
        map_queue: queue.Queue,
        root_dir: str = "tests/data",
        osu_dir: Optional[str] = None,
    ):
        self.map_queue = map_queue
        self.root_dir = root_dir
        self.osu_dir = osu_dir
        self.root: Optional[tk.Tk] = None
        self.tree: Optional[ttk.Treeview] = None
        self.all_entries: List[MapEntry] = []
        self.filtered_entries: List[MapEntry] = []
        self.search_var: Optional[tk.StringVar] = None
        self.category_var: Optional[tk.StringVar] = None
        self.only_replays_var: Optional[tk.BooleanVar] = None
        self.info_lbl: Optional[tk.Label] = None
        self.cat_combo: Optional[ttk.Combobox] = None

    def start(self):
        self.root = tk.Tk()
        self.root.title("Rosea - Выбор карты & Реплеи")
        self.root.geometry("1080x600")
        self.root.minsize(800, 480)
        self.root.configure(bg="#12141c")

        # Hide on close instead of destroying to keep thread alive
        self.root.protocol("WM_DELETE_WINDOW", self.hide)

        # Style configuration
        self.setup_styles()

        # Build UI layout
        self.build_ui()

        # Initial map scan
        self.refresh_maps()

        # Keyboard shortcuts
        self.root.bind("<Escape>", lambda e: self.hide())
        self.root.bind("<Return>", lambda e: self.on_load_clicked())

        # Start mainloop
        self.root.mainloop()

    def setup_styles(self):
        style = ttk.Style(self.root)
        try:
            style.theme_use("clam")
        except Exception:
            pass

        # Dark treeview colors
        style.configure(
            "Dark.Treeview",
            background="#161824",
            foreground="#f1f5f9",
            fieldbackground="#161824",
            font=("Segoe UI", 10),
            rowheight=26,
            borderwidth=0,
        )
        style.map(
            "Dark.Treeview",
            background=[("selected", "#be185d")],
            foreground=[("selected", "#ffffff")],
        )

        style.configure(
            "Dark.Treeview.Heading",
            background="#1e2235",
            foreground="#94a3b8",
            font=("Segoe UI", 9, "bold"),
            relief="flat",
            padding=5,
        )
        style.map(
            "Dark.Treeview.Heading",
            background=[("active", "#2d334d")],
            foreground=[("active", "#ffffff")],
        )

        # Scrollbar
        style.configure(
            "Dark.Vertical.TScrollbar",
            background="#1e2235",
            troughcolor="#12141c",
            arrowcolor="#94a3b8",
            borderwidth=0,
        )

        # Combobox
        style.configure(
            "Dark.TCombobox",
            background="#1e2235",
            fieldbackground="#1e2235",
            foreground="#f1f5f9",
            arrowcolor="#f1f5f9",
            padding=4,
        )

    def build_ui(self):
        # 1. Header Bar
        hdr_frame = tk.Frame(self.root, bg="#12141c", pady=8, padx=14)
        hdr_frame.pack(fill=tk.X)

        title_lbl = tk.Label(
            hdr_frame,
            text="🎵 Выбор карты & Реплеи osu! для визуализатора Rosea",
            font=("Segoe UI", 12, "bold"),
            bg="#12141c",
            fg="#f1f5f9",
        )
        title_lbl.pack(side=tk.LEFT)

        sub_lbl = tk.Label(
            hdr_frame,
            text="Автоматически загружает карту + повтор из osu! stable и tests/data",
            font=("Segoe UI", 9),
            bg="#12141c",
            fg="#94a3b8",
        )
        sub_lbl.pack(side=tk.RIGHT, padx=5)

        # 2. Search and Filter Bar
        filter_frame = tk.Frame(self.root, bg="#181b28", padx=12, pady=10)
        filter_frame.pack(fill=tk.X, padx=14, pady=4)

        # Search icon & entry
        s_icon = tk.Label(
            filter_frame,
            text="🔍",
            bg="#181b28",
            fg="#94a3b8",
            font=("Segoe UI", 11),
        )
        s_icon.pack(side=tk.LEFT, padx=(0, 6))

        self.search_var = tk.StringVar()
        self.search_var.trace_add("write", lambda *args: self.apply_filter())
        search_entry = tk.Entry(
            filter_frame,
            textvariable=self.search_var,
            bg="#222738",
            fg="#ffffff",
            insertbackground="#ffffff",
            font=("Segoe UI", 10),
            relief=tk.FLAT,
            highlightthickness=1,
            highlightbackground="#2d334d",
            highlightcolor="#ec4899",
        )
        search_entry.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=(0, 10), ipady=4)

        # Only replays checkbox
        self.only_replays_var = tk.BooleanVar(value=False)
        chk = tk.Checkbutton(
            filter_frame,
            text="🟣 Только с реплеем",
            variable=self.only_replays_var,
            bg="#181b28",
            fg="#f1f5f9",
            selectcolor="#222738",
            activebackground="#181b28",
            activeforeground="#ec4899",
            font=("Segoe UI", 9, "bold"),
            cursor="hand2",
            command=self.apply_filter,
        )
        chk.pack(side=tk.LEFT, padx=(4, 10))

        # Category Filter
        cat_lbl = tk.Label(
            filter_frame,
            text="Папка / Источник:",
            bg="#181b28",
            fg="#94a3b8",
            font=("Segoe UI", 9, "bold"),
        )
        cat_lbl.pack(side=tk.LEFT, padx=(5, 6))

        self.category_var = tk.StringVar(value="Все карты")
        self.cat_combo = ttk.Combobox(
            filter_frame,
            textvariable=self.category_var,
            state="readonly",
            style="Dark.TCombobox",
            width=26,
        )
        self.cat_combo.pack(side=tk.LEFT, padx=(0, 10))
        self.cat_combo.bind("<<ComboboxSelected>>", lambda e: self.apply_filter())

        # Rescan Button
        rescan_btn = tk.Button(
            filter_frame,
            text="🔄 Обновить",
            bg="#272c40",
            fg="#cbd5e1",
            activebackground="#333a54",
            activeforeground="#ffffff",
            font=("Segoe UI", 9),
            relief=tk.FLAT,
            padx=10,
            pady=3,
            cursor="hand2",
            command=self.refresh_maps,
        )
        rescan_btn.pack(side=tk.RIGHT)

        # 3. Main Treeview
        tree_frame = tk.Frame(self.root, bg="#12141c")
        tree_frame.pack(fill=tk.BOTH, expand=True, padx=14, pady=6)

        cols = ("title", "replay", "category", "cs_ar", "objects", "engine")
        self.tree = ttk.Treeview(
            tree_frame,
            columns=cols,
            show="headings",
            style="Dark.Treeview",
            selectmode="browse",
        )

        self.tree.heading("title", text="Название карты [Сложность]", anchor=tk.W)
        self.tree.heading("replay", text="Повтор / Игрок (.osr)", anchor=tk.W)
        self.tree.heading("category", text="Папка / Источник", anchor=tk.W)
        self.tree.heading("cs_ar", text="CS / AR", anchor=tk.CENTER)
        self.tree.heading("objects", text="Ноты", anchor=tk.CENTER)
        self.tree.heading("engine", text="Движок Rust", anchor=tk.CENTER)

        self.tree.column("title", width=360, minwidth=240, anchor=tk.W)
        self.tree.column("replay", width=230, minwidth=140, anchor=tk.W)
        self.tree.column("category", width=130, minwidth=90, anchor=tk.W)
        self.tree.column("cs_ar", width=75, minwidth=60, anchor=tk.CENTER)
        self.tree.column("objects", width=65, minwidth=50, anchor=tk.CENTER)
        self.tree.column("engine", width=105, minwidth=85, anchor=tk.CENTER)

        scrollbar = ttk.Scrollbar(
            tree_frame,
            orient=tk.VERTICAL,
            command=self.tree.yview,
            style="Dark.Vertical.TScrollbar",
        )
        self.tree.configure(yscrollcommand=scrollbar.set)

        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)

        self.tree.bind("<Double-1>", lambda e: self.on_load_clicked())
        self.tree.bind("<<TreeviewSelect>>", self.on_select_item)

        # Tags for colored text
        self.tree.tag_configure("rust_ok", foreground="#4ade80")
        self.tree.tag_configure("py_only", foreground="#38bdf8")

        # 4. Bottom Action & Status Bar
        bot_frame = tk.Frame(self.root, bg="#12141c", pady=10, padx=14)
        bot_frame.pack(fill=tk.X)

        self.info_lbl = tk.Label(
            bot_frame,
            text="Выберите карту и дважды кликните для воспроизведения вместе с реплеем",
            font=("Segoe UI", 9),
            bg="#12141c",
            fg="#94a3b8",
            anchor=tk.W,
        )
        self.info_lbl.pack(side=tk.LEFT, fill=tk.X, expand=True)

        load_btn = tk.Button(
            bot_frame,
            text="▶ Загрузить в визуализатор (Enter)",
            bg="#ec4899",
            fg="#ffffff",
            activebackground="#db2777",
            activeforeground="#ffffff",
            font=("Segoe UI", 10, "bold"),
            relief=tk.FLAT,
            padx=16,
            pady=6,
            cursor="hand2",
            command=self.on_load_clicked,
        )
        load_btn.pack(side=tk.RIGHT)

    def refresh_maps(self):
        """Rescan tests/data and osu! stable pairs and populate table."""
        self.all_entries = scan_all_maps(self.root_dir, osu_dir=self.osu_dir)

        # Group categories cleanly
        stable_cats = []
        test_cats = []
        for e in self.all_entries:
            if e.category.startswith("osu!"):
                if e.category not in stable_cats:
                    stable_cats.append(e.category)
            else:
                if e.category not in test_cats:
                    test_cats.append(e.category)

        special_cats = ["Все карты", "🎮 Пары с реплеями (.osr)"]
        all_cats = special_cats + sorted(stable_cats) + sorted(test_cats)
        if self.cat_combo:
            self.cat_combo["values"] = all_cats

        self.apply_filter()

    def apply_filter(self):
        """Filter table based on search input, selected category, and replay toggle."""
        if not self.tree:
            return

        query = self.search_var.get() if self.search_var else ""
        selected_cat = self.category_var.get() if self.category_var else "Все карты"
        only_replays = self.only_replays_var.get() if self.only_replays_var else False

        self.tree.delete(*self.tree.get_children())
        self.filtered_entries = []

        for entry in self.all_entries:
            if only_replays and not entry.has_replay:
                continue

            if selected_cat == "🎮 Пары с реплеями (.osr)":
                if not entry.has_replay:
                    continue
            elif selected_cat != "Все карты" and entry.category != selected_cat:
                continue

            if not entry.matches(query):
                continue

            self.filtered_entries.append(entry)

            engine_text = "🟢 Rust MVP" if entry.rust_supported else "🔵 Python Full"
            engine_tag = "rust_ok" if entry.rust_supported else "py_only"

            if entry.has_replay:
                if entry.player_name:
                    if entry.replay_score >= 1_000_000:
                        score_str = f"{entry.replay_score/1_000_000:.2f}M"
                    elif entry.replay_score >= 1_000:
                        score_str = f"{entry.replay_score//1000}k"
                    else:
                        score_str = str(entry.replay_score)
                    combo_str = f"{entry.replay_combo}x" if entry.replay_combo > 0 else ""
                    combo_part = f", {combo_str}" if combo_str else ""
                    replay_text = f"🟣 {entry.player_name} ({score_str}{combo_part})"
                else:
                    replay_text = "🟣 Есть .osr"
            else:
                replay_text = "—"

            cs_ar_text = f"{entry.cs:.1f} / {entry.ar:.1f}"
            obj_text = f"{entry.objects_count}"

            item_id = self.tree.insert(
                "",
                tk.END,
                values=(
                    entry.display_title,
                    replay_text,
                    entry.category,
                    cs_ar_text,
                    obj_text,
                    engine_text,
                ),
                tags=(engine_tag,),
            )
            self.tree.set(item_id, "title", entry.display_title)

        count = len(self.filtered_entries)
        if self.info_lbl:
            replays_cnt = sum(1 for e in self.filtered_entries if e.has_replay)
            self.info_lbl.config(
                text=f"Найдено: {count} карт ({replays_cnt} с реплеями) из {len(self.all_entries)} | Двойной клик для запуска"
            )

    def on_select_item(self, event=None):
        selected = self.tree.selection()
        if not selected:
            return
        idx = self.tree.index(selected[0])
        if 0 <= idx < len(self.filtered_entries):
            entry = self.filtered_entries[idx]
            engine_str = "Поддерживается Rust" if entry.rust_supported else "Python Standalone"
            if entry.has_replay:
                replay_str = f"Реплей: {entry.player_name} (Счёт {entry.replay_score:,}, Комбо {entry.replay_combo})"
            else:
                replay_str = "Без реплея"
            if self.info_lbl:
                self.info_lbl.config(
                    text=f"Выбрано: {entry.display_title} | {engine_str} | {replay_str}"
                )

    def on_load_clicked(self):
        selected = self.tree.selection()
        if not selected:
            return
        idx = self.tree.index(selected[0])
        if 0 <= idx < len(self.filtered_entries):
            entry = self.filtered_entries[idx]
            # Send tuple (file_path, replay_path) so visualizer loads both map & replay
            self.map_queue.put((entry.file_path, entry.replay_path))
            rep_text = f" + Реплей ({entry.player_name})" if entry.replay_path else ""
            if self.info_lbl:
                self.info_lbl.config(
                    text=f"✔ Отправлено в плеер: {entry.display_title}{rep_text}"
                )

    def show(self):
        """Show and bring window to front in a thread-safe manner."""
        if self.root:
            self.root.after(0, self._do_show)

    def _do_show(self):
        if self.root:
            self.root.deiconify()
            self.root.lift()
            self.root.focus_force()

    def hide(self):
        """Hide window without terminating thread."""
        if self.root:
            self.root.after(0, self.root.withdraw)


class MapBrowserManager:
    """Thread-safe manager for the non-blocking map browser companion window."""

    def __init__(
        self,
        root_dir: str = "tests/data",
        osu_dir: Optional[str] = None,
    ):
        self.map_queue: queue.Queue = queue.Queue()
        self.root_dir = root_dir
        self.osu_dir = osu_dir
        self.app: Optional[MapBrowserApp] = None
        self.thread: Optional[threading.Thread] = None

    def start(self):
        self.thread = threading.Thread(target=self._run_thread, daemon=True)
        self.thread.start()

    def _run_thread(self):
        self.app = MapBrowserApp(
            self.map_queue,
            root_dir=self.root_dir,
            osu_dir=self.osu_dir,
        )
        self.app.start()

    def show(self):
        if self.app:
            self.app.show()

    def hide(self):
        if self.app:
            self.app.hide()

    def get_selected_map(self) -> Optional[Tuple[str, Optional[str]]]:
        """Poll for newly selected map and replay paths from queue without blocking."""
        try:
            return self.map_queue.get_nowait()
        except queue.Empty:
            return None


if __name__ == "__main__":
    import time
    print("Testing MapBrowser standalone...")
    entries = scan_all_maps()
    print(f"Total maps scanned: {len(entries)}")
    with_rep = sum(1 for e in entries if e.has_replay)
    print(f"Maps with replay: {with_rep}")
    for e in entries[:8]:
        rep_p = f"Replay: {e.player_name}" if e.has_replay else "No replay"
        print(f"  [{e.category}] {e.display_title} -> {rep_p}")
