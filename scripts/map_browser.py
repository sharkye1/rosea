#!/usr/bin/env python3
"""
Rosea Map Browser - Non-blocking companion window for selecting beatmaps.
Scans tests/data/ for all available .osu beatmaps, displays metadata,
difficulty stats, Rust compatibility, and replay availability with real-time search.
"""

import glob
import os
import queue
import sys
import threading
import tkinter as tk
from dataclasses import dataclass
from tkinter import ttk
from typing import Any, Callable, Dict, List, Optional


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
        )


def fast_scan_beatmap(path: str) -> Optional[MapEntry]:
    """Quickly extract metadata and difficulty from .osu file without full simulation."""
    try:
        norm_path = path.replace("\\", "/")
        parts = norm_path.split("/")
        category = parts[-2] if len(parts) >= 2 else "other"
        filename = os.path.basename(path)

        # Check for adjacent .osr replay
        candidate_osr = os.path.splitext(path)[0] + ".osr"
        has_replay = os.path.isfile(candidate_osr)

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
                                # Check curve type in tail
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

        # Rust rosea-mvp-v1 supports circles and L-sliders
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
        )
    except Exception:
        return None


def scan_all_maps(root_dir: str = "tests/data") -> List[MapEntry]:
    """Scan root directory for all .osu beatmaps."""
    pattern = os.path.join(root_dir, "**", "*.osu")
    files = sorted(glob.glob(pattern, recursive=True))
    entries = []
    for f in files:
        entry = fast_scan_beatmap(f)
        if entry:
            entries.append(entry)
    return entries


class MapBrowserApp:
    def __init__(
        self,
        map_queue: queue.Queue,
        root_dir: str = "tests/data",
        on_close_callback: Optional[Callable[[], None]] = None,
    ):
        self.map_queue = map_queue
        self.root_dir = root_dir
        self.on_close_callback = on_close_callback
        self.all_entries: List[MapEntry] = []
        self.filtered_entries: List[MapEntry] = []

        self.root: Optional[tk.Tk] = None
        self.tree: Optional[ttk.Treeview] = None
        self.search_var: Optional[tk.StringVar] = None
        self.category_var: Optional[tk.StringVar] = None
        self.status_lbl: Optional[tk.Label] = None
        self.info_lbl: Optional[tk.Label] = None
        self.cat_combo: Optional[ttk.Combobox] = None

    def start(self):
        """Build and launch the Tkinter window loop."""
        self.root = tk.Tk()
        self.root.title("Rosea - Выбор карты (tests/data)")
        self.root.geometry("860x540")
        self.root.minsize(700, 400)
        self.root.configure(bg="#12141c")

        # Handle window X button: hide instead of destroy
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
            text="🎵 Выбор карты для визуализатора Rosea",
            font=("Segoe UI", 12, "bold"),
            bg="#12141c",
            fg="#f1f5f9",
        )
        title_lbl.pack(side=tk.LEFT)

        sub_lbl = tk.Label(
            hdr_frame,
            text="Мгновенное переключение на лету в реальном времени",
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

        # Category Filter
        cat_lbl = tk.Label(
            filter_frame,
            text="Папка:",
            bg="#181b28",
            fg="#94a3b8",
            font=("Segoe UI", 9, "bold"),
        )
        cat_lbl.pack(side=tk.LEFT, padx=(5, 6))

        self.category_var = tk.StringVar(value="Все папки")
        self.cat_combo = ttk.Combobox(
            filter_frame,
            textvariable=self.category_var,
            state="readonly",
            style="Dark.TCombobox",
            width=20,
        )
        self.cat_combo.pack(side=tk.LEFT, padx=(0, 10))
        self.cat_combo.bind("<<ComboboxSelected>>", lambda e: self.apply_filter())

        # Rescan Button
        rescan_btn = tk.Button(
            filter_frame,
            text="🔄 Обновить список",
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

        cols = ("title", "category", "cs_ar", "objects", "engine", "replay")
        self.tree = ttk.Treeview(
            tree_frame,
            columns=cols,
            show="headings",
            style="Dark.Treeview",
            selectmode="browse",
        )

        self.tree.heading("title", text="Название карты [Сложность]", anchor=tk.W)
        self.tree.heading("category", text="Папка", anchor=tk.W)
        self.tree.heading("cs_ar", text="CS / AR", anchor=tk.CENTER)
        self.tree.heading("objects", text="Объекты", anchor=tk.CENTER)
        self.tree.heading("engine", text="Движок Rust", anchor=tk.CENTER)
        self.tree.heading("replay", text="Реплей .osr", anchor=tk.CENTER)

        self.tree.column("title", width=340, minwidth=220, anchor=tk.W)
        self.tree.column("category", width=120, minwidth=80, anchor=tk.W)
        self.tree.column("cs_ar", width=80, minwidth=60, anchor=tk.CENTER)
        self.tree.column("objects", width=80, minwidth=60, anchor=tk.CENTER)
        self.tree.column("engine", width=110, minwidth=90, anchor=tk.CENTER)
        self.tree.column("replay", width=90, minwidth=70, anchor=tk.CENTER)

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
            text="Выберите карту и дважды кликните для воспроизведения",
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
        """Rescan tests/data directory and populate table."""
        self.all_entries = scan_all_maps(self.root_dir)

        # Update categories combobox
        cats = sorted(list(set(e.category for e in self.all_entries)))
        self.cat_combo["values"] = ["Все папки"] + cats

        self.apply_filter()

    def apply_filter(self):
        """Filter table based on search input and selected category."""
        if not self.tree:
            return

        query = self.search_var.get() if self.search_var else ""
        selected_cat = self.category_var.get() if self.category_var else "Все папки"

        self.tree.delete(*self.tree.get_children())
        self.filtered_entries = []

        for entry in self.all_entries:
            if selected_cat != "Все папки" and entry.category != selected_cat:
                continue
            if not entry.matches(query):
                continue

            self.filtered_entries.append(entry)

            engine_text = "🟢 Rust MVP" if entry.rust_supported else "🔵 Python Full"
            engine_tag = "rust_ok" if entry.rust_supported else "py_only"
            replay_text = "🟣 Есть .osr" if entry.has_replay else "—"

            cs_ar_text = f"{entry.cs:.1f} / {entry.ar:.1f}"
            obj_text = f"{entry.objects_count} нот"

            item_id = self.tree.insert(
                "",
                tk.END,
                values=(
                    entry.display_title,
                    entry.category,
                    cs_ar_text,
                    obj_text,
                    engine_text,
                    replay_text,
                ),
                tags=(engine_tag,),
            )
            # Store file_path in a hidden mapping or selection tag
            self.tree.set(item_id, "title", entry.display_title)

        count = len(self.filtered_entries)
        if self.info_lbl:
            self.info_lbl.config(
                text=f"Доступно карт: {count} из {len(self.all_entries)} | Двойной клик для мгновенного запуска"
            )

    def on_select_item(self, event=None):
        selected = self.tree.selection()
        if not selected:
            return
        idx = self.tree.index(selected[0])
        if 0 <= idx < len(self.filtered_entries):
            entry = self.filtered_entries[idx]
            engine_str = "Поддерживается Rust-парсером" if entry.rust_supported else "Python Standalone (Безье/Спиннеры)"
            replay_str = "Реплей подключён" if entry.has_replay else "Без реплея"
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
            self.map_queue.put(entry.file_path)
            if self.info_lbl:
                self.info_lbl.config(
                    text=f"✔ Отправлено в плеер: {entry.filename} ({entry.display_title})"
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

    def __init__(self, root_dir: str = "tests/data"):
        self.map_queue: queue.Queue = queue.Queue()
        self.root_dir = root_dir
        self.app: Optional[MapBrowserApp] = None
        self.thread: Optional[threading.Thread] = None

    def start(self):
        self.thread = threading.Thread(target=self._run_thread, daemon=True)
        self.thread.start()

    def _run_thread(self):
        self.app = MapBrowserApp(self.map_queue, root_dir=self.root_dir)
        self.app.start()

    def show(self):
        if self.app:
            self.app.show()

    def hide(self):
        if self.app:
            self.app.hide()

    def get_selected_map(self) -> Optional[str]:
        """Poll for newly selected map path from queue without blocking."""
        try:
            return self.map_queue.get_nowait()
        except queue.Empty:
            return None
