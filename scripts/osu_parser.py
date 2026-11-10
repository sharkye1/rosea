"""
osu! beatmap parser supporting full beatmap specifications:
- Hit Circles
- Sliders: Linear ('L'), Bézier ('B'), Perfect Circle ('P'), Catmull-Rom ('C')
- Spinners
- Timing Points (Uninherited & Inherited / SV)
- Stacking calculation
- Difficulty, Metadata, Colours
"""

import hashlib
import math
from typing import List, Tuple, Dict, Any, Optional

Point = Tuple[float, float]


def distance(p1: Point, p2: Point) -> float:
    return math.hypot(p2[0] - p1[0], p2[1] - p1[1])


def de_casteljau(pts: List[Point], t: float) -> Point:
    """Evaluate a Bézier curve of any degree at parameter t in [0, 1]."""
    cur = list(pts)
    n = len(cur)
    for r in range(1, n):
        for i in range(n - r):
            cur[i] = (
                cur[i][0] * (1.0 - t) + cur[i + 1][0] * t,
                cur[i][1] * (1.0 - t) + cur[i + 1][1] * t,
            )
    return cur[0]


def generate_bezier_segment(controls: List[Point], step: float = 2.5) -> List[Point]:
    """Sample a single Bézier segment."""
    n = len(controls)
    if n == 0:
        return []
    if n == 1:
        return [controls[0]]
    if n == 2:
        # Straight line
        p0, p1 = controls[0], controls[1]
        dist = distance(p0, p1)
        steps = max(2, int(dist / step))
        return [
            (p0[0] + (p1[0] - p0[0]) * (i / steps), p0[1] + (p1[1] - p0[1]) * (i / steps))
            for i in range(steps + 1)
        ]

    # Estimate length by control polygon
    poly_len = sum(distance(controls[i], controls[i + 1]) for i in range(n - 1))
    steps = max(8, int(poly_len / step))
    return [de_casteljau(controls, i / steps) for i in range(steps + 1)]


def generate_bezier_curve(controls: List[Point]) -> List[Point]:
    """
    Split control points on identical consecutive points, evaluate each segment,
    and concatenate them.
    """
    if not controls:
        return []
    segments: List[List[Point]] = []
    current: List[Point] = [controls[0]]

    for i in range(1, len(controls)):
        p = controls[i]
        prev = controls[i - 1]
        if abs(p[0] - prev[0]) < 1e-4 and abs(p[1] - prev[1]) < 1e-4:
            # End current segment, start new one at this duplicate point
            segments.append(current)
            current = [p]
        else:
            current.append(p)
    segments.append(current)

    out_points: List[Point] = []
    for seg in segments:
        sampled = generate_bezier_segment(seg)
        if not out_points:
            out_points.extend(sampled)
        else:
            # Avoid duplicate vertex at junction
            out_points.extend(sampled[1:])
    return out_points


def generate_linear_polyline(points: List[Point], step: float = 2.5) -> List[Point]:
    """Sample linear segments connecting points sequentially."""
    if not points:
        return []
    if len(points) == 1:
        return list(points)
    out: List[Point] = []
    for i in range(len(points) - 1):
        seg = generate_bezier_segment([points[i], points[i + 1]], step)
        if not out:
            out.extend(seg)
        else:
            out.extend(seg[1:])
    return out


def generate_circular_arc(a: Point, b: Point, c: Point, step: float = 2.5) -> List[Point]:
    """
    Generate circumscribed circular arc passing through 3 points a, b, c.
    Falls back to linear polyline if points are collinear or circle is degenerate.
    """
    d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]))
    if abs(d) < 1e-4:
        # Collinear fallback: straight lines a -> b and b -> c (matching osu!stable PathApproximator)
        return generate_linear_polyline([a, b, c], step)

    a_sq = a[0] ** 2 + a[1] ** 2
    b_sq = b[0] ** 2 + b[1] ** 2
    c_sq = c[0] ** 2 + c[1] ** 2

    cx = (a_sq * (b[1] - c[1]) + b_sq * (c[1] - a[1]) + c_sq * (a[1] - b[1])) / d
    cy = (a_sq * (c[0] - b[0]) + b_sq * (a[0] - c[0]) + c_sq * (b[0] - a[0])) / d
    r = math.hypot(a[0] - cx, a[1] - cy)

    theta_a = math.atan2(a[1] - cy, a[0] - cx)
    theta_b = math.atan2(b[1] - cy, b[0] - cx)
    theta_c = math.atan2(c[1] - cy, c[0] - cx)

    # Calculate angular distance relative to theta_a
    d_ab = (theta_b - theta_a) % (2.0 * math.pi)
    d_ac = (theta_c - theta_a) % (2.0 * math.pi)

    if d_ab < d_ac:
        # Clockwise
        total_angle = d_ac
    else:
        # Counter-clockwise
        total_angle = d_ac - 2.0 * math.pi

    arc_len = abs(r * total_angle)
    steps = max(8, int(arc_len / step))

    points = []
    for i in range(steps + 1):
        angle = theta_a + total_angle * (i / steps)
        points.append((cx + r * math.cos(angle), cy + r * math.sin(angle)))
    return points


def generate_catmull_curve(controls: List[Point], step: float = 2.5) -> List[Point]:
    """Generate Centripetal Catmull-Rom spline with clamped endpoints (matching osu!stable)."""
    n = len(controls)
    if n < 2:
        return list(controls)
    if n == 2:
        return generate_bezier_segment(controls, step)

    points: List[Point] = []
    for i in range(n - 1):
        # In osu!stable, endpoint control points are clamped: v1 = i > 0 ? c[i-1] : c[i]
        p0 = controls[i - 1] if i > 0 else controls[i]
        p1 = controls[i]
        p2 = controls[i + 1]
        p3 = controls[i + 2] if i + 2 < n else controls[i + 1]

        approx_len = distance(p1, p2)
        steps = max(4, int(approx_len / step))
        for step_idx in range(steps if i < n - 2 else steps + 1):
            t = step_idx / steps
            t2 = t * t
            t3 = t2 * t
            x = 0.5 * (
                (2 * p1[0])
                + (-p0[0] + p2[0]) * t
                + (2 * p0[0] - 5 * p1[0] + 4 * p2[0] - p3[0]) * t2
                + (-p0[0] + 3 * p1[0] - 3 * p2[0] + p3[0]) * t3
            )
            y = 0.5 * (
                (2 * p1[1])
                + (-p0[1] + p2[1]) * t
                + (2 * p0[1] - 5 * p1[1] + 4 * p2[1] - p3[1]) * t2
                + (-p0[1] + 3 * p1[1] - 3 * p2[1] + p3[1]) * t3
            )
            points.append((x, y))
    return points


def generate_slider_path(
    curve_type: str, controls: List[Point], declared_length: float
) -> Tuple[List[Point], List[float], float]:
    """
    Generate the sampled path for a slider curve and truncate/extend it to declared_length.
    Returns:
      (points, cumulative_distances, total_length)
    """
    if len(controls) < 2:
        pts = list(controls)
        return pts, [0.0] * len(pts), 0.0

    raw_points: List[Point] = []
    if curve_type == 'L':
        # Linear polyline
        for i in range(len(controls) - 1):
            seg = generate_bezier_segment([controls[i], controls[i + 1]])
            if not raw_points:
                raw_points.extend(seg)
            else:
                raw_points.extend(seg[1:])
    elif curve_type == 'P':
        if len(controls) == 3:
            raw_points = generate_circular_arc(controls[0], controls[1], controls[2])
        else:
            raw_points = generate_bezier_curve(controls)
    elif curve_type == 'C':
        raw_points = generate_catmull_curve(controls)
    else:  # 'B' or default
        raw_points = generate_bezier_curve(controls)

    if not raw_points:
        raw_points = list(controls)

    # Calculate cumulative distances
    cum_dist = [0.0]
    for i in range(1, len(raw_points)):
        d = distance(raw_points[i - 1], raw_points[i])
        cum_dist.append(cum_dist[-1] + d)

    total_len = cum_dist[-1]

    if declared_length <= 0.0:
        return [raw_points[0]], [0.0], 0.0

    # Truncate or extend to declared_length
    if abs(total_len - declared_length) < 1e-4:
        return raw_points, cum_dist, declared_length

    if total_len > declared_length:
        truncated_points: List[Point] = []
        truncated_cum_dist: List[float] = []

        for i in range(len(raw_points)):
            if cum_dist[i] <= declared_length:
                truncated_points.append(raw_points[i])
                truncated_cum_dist.append(cum_dist[i])
            else:
                # Interpolate last point
                prev_dist = cum_dist[i - 1]
                seg_dist = cum_dist[i] - prev_dist
                if seg_dist > 1e-6:
                    frac = (declared_length - prev_dist) / seg_dist
                    p_prev = raw_points[i - 1]
                    p_curr = raw_points[i]
                    final_pt = (
                        p_prev[0] + (p_curr[0] - p_prev[0]) * frac,
                        p_prev[1] + (p_curr[1] - p_prev[1]) * frac,
                    )
                else:
                    final_pt = raw_points[i]
                truncated_points.append(final_pt)
                truncated_cum_dist.append(declared_length)
                break
        return truncated_points, truncated_cum_dist, declared_length

    # total_len < declared_length: extend along last segment tangent (matching osu!stable)
    if len(raw_points) >= 2:
        p_prev = raw_points[-2]
        p_last = raw_points[-1]
        seg_d = distance(p_prev, p_last)
        needed = declared_length - total_len
        if seg_d > 1e-6:
            dx = (p_last[0] - p_prev[0]) / seg_d
            dy = (p_last[1] - p_prev[1]) / seg_d
            ext_pt = (p_last[0] + dx * needed, p_last[1] + dy * needed)
        else:
            ext_pt = p_last
        extended_points = list(raw_points) + [ext_pt]
        extended_cum = list(cum_dist) + [declared_length]
        return extended_points, extended_cum, declared_length

    return raw_points, cum_dist, total_len


class TimingPoint:
    def __init__(
        self,
        line: int,
        time: float,
        beat_length: float,
        meter: int,
        sample_set: int,
        sample_index: int,
        volume: int,
        uninherited: bool,
        effects: int,
    ):
        self.line = line
        self.time = time
        self.beat_length = beat_length
        self.meter = meter
        self.sample_set = sample_set
        self.sample_index = sample_index
        self.volume = volume
        self.uninherited = uninherited
        self.effects = effects

    @property
    def bpm(self) -> float:
        if self.uninherited and self.beat_length > 0:
            return 60000.0 / self.beat_length
        return 0.0

    @property
    def sv_multiplier(self) -> float:
        if not self.uninherited and self.beat_length < 0:
            return max(0.1, min(10.0, -100.0 / self.beat_length))
        return 1.0


class HitObject:
    def __init__(
        self,
        obj_id: int,
        line: int,
        x: float,
        y: float,
        time: float,
        flags: int,
        hit_sound: int,
        kind: str,
    ):
        self.id = obj_id
        self.line = line
        self.x = x
        self.y = y
        self.time = time
        self.flags = flags
        self.hit_sound = hit_sound
        self.kind = kind  # 'circle', 'slider', 'spinner'

        # Combo info
        self.is_new_combo = bool(flags & 4)
        self.combo_index = 0
        self.combo_number = 1

        # Stacking
        self.stack_height = 0
        self.stacked_x = x
        self.stacked_y = y

        # Lifecycle times
        self.end_time = time

        # Slider attributes
        self.curve_type: str = 'L'
        self.controls: List[Point] = []
        self.spans: int = 1
        self.declared_length: float = 0.0
        self.path_points: List[Point] = []
        self.path_cum_dist: List[float] = []
        self.path_total_length: float = 0.0
        self.span_duration: float = 0.0
        self.total_duration: float = 0.0
        self.tick_distances: List[float] = []

        # Spinner attributes
        self.spinner_end_time: float = time

    def position_at_progress(self, progress: float) -> Point:
        """Get (x, y) along slider path at progress in [0, 1]."""
        if not self.path_points:
            return (self.stacked_x, self.stacked_y)
        progress = max(0.0, min(1.0, progress))
        target_dist = progress * self.path_total_length
        if target_dist <= 0:
            pt = self.path_points[0]
            dx = self.stacked_x - self.x
            dy = self.stacked_y - self.y
            return (pt[0] + dx, pt[1] + dy)
        if target_dist >= self.path_total_length:
            pt = self.path_points[-1]
            dx = self.stacked_x - self.x
            dy = self.stacked_y - self.y
            return (pt[0] + dx, pt[1] + dy)

        # Binary search
        low = 0
        high = len(self.path_cum_dist) - 1
        while low <= high:
            mid = (low + high) // 2
            if self.path_cum_dist[mid] < target_dist:
                low = mid + 1
            else:
                high = mid - 1
        idx = low
        if idx <= 0:
            pt = self.path_points[0]
        elif idx >= len(self.path_points):
            pt = self.path_points[-1]
        else:
            d0 = self.path_cum_dist[idx - 1]
            d1 = self.path_cum_dist[idx]
            seg_len = d1 - d0
            t = (target_dist - d0) / seg_len if seg_len > 1e-6 else 0.0
            p0 = self.path_points[idx - 1]
            p1 = self.path_points[idx]
            pt = (p0[0] + (p1[0] - p0[0]) * t, p0[1] + (p1[1] - p0[1]) * t)

        dx = self.stacked_x - self.x
        dy = self.stacked_y - self.y
        return (pt[0] + dx, pt[1] + dy)


class Beatmap:
    def __init__(self):
        self.version = 14
        self.mode = 0

        # General
        self.audio_filename = ""
        self.audio_lead_in = 0
        self.preview_time = 0
        self.stack_leniency = 0.7

        # Metadata
        self.title = ""
        self.title_unicode: Optional[str] = None
        self.artist = ""
        self.artist_unicode: Optional[str] = None
        self.creator = ""
        self.version_name = ""
        self.source = ""
        self.tags = ""
        self.beatmap_id: Optional[int] = None
        self.beatmapset_id: Optional[int] = None

        # Difficulty
        self.cs = 4.0
        self.od = 8.0
        self.hp = 5.0
        self.ar = 9.0
        self.slider_multiplier = 1.4
        self.slider_tick_rate = 1.0

        # Timing, Events & Objects
        self.timing_points: List[TimingPoint] = []
        self.hit_objects: List[HitObject] = []
        self.breaks: List[Tuple[float, float]] = []
        self.combo_colours: List[Tuple[int, int, int]] = [
            (255, 192, 0),
            (0, 202, 0),
            (18, 124, 255),
            (242, 24, 57),
        ]
        self.slider_track_override: Optional[Tuple[int, int, int]] = None
        self.slider_border: Optional[Tuple[int, int, int]] = None
        self.beatmap_md5: str = ""
        self.rust_warnings: List[Dict[str, Any]] = []

    @property
    def circle_radius(self) -> float:
        """Hit circle radius in osu! pixels."""
        return 54.4 - 4.48 * self.cs

    @property
    def ar_preempt_fadein(self) -> Tuple[float, float]:
        """Returns (preempt_ms, fadein_ms)."""
        ar = self.ar
        if ar < 5.0:
            preempt = 1200.0 + 600.0 * (5.0 - ar) / 5.0
            fade_in = 800.0 + 400.0 * (5.0 - ar) / 5.0
        elif ar == 5.0:
            preempt = 1200.0
            fade_in = 800.0
        else:
            preempt = 1200.0 - 750.0 * (ar - 5.0) / 5.0
            fade_in = 800.0 - 500.0 * (ar - 5.0) / 5.0
        return preempt, fade_in

    def get_timing_at(self, time_ms: float) -> Tuple[Optional[TimingPoint], Optional[TimingPoint]]:
        """Returns (active_uninherited_tp, active_tp)."""
        active_uninherited = None
        active_any = None
        for tp in self.timing_points:
            if tp.time <= time_ms:
                active_any = tp
                if tp.uninherited:
                    active_uninherited = tp
            else:
                break
        if active_uninherited is None:
            # Fallback to first uninherited
            for tp in self.timing_points:
                if tp.uninherited:
                    active_uninherited = tp
                    break
        if active_any is None:
            active_any = active_uninherited
        return active_uninherited, active_any


def parse_osu_beatmap(file_path: str) -> Beatmap:
    """Parse complete .osu beatmap file."""
    with open(file_path, "rb") as f:
        raw_bytes = f.read()

    bm = Beatmap()
    bm.beatmap_md5 = hashlib.md5(raw_bytes).hexdigest()
    lines = raw_bytes.decode("utf-8", errors="replace").splitlines()

    # Parse header
    for line in lines:
        s = line.strip().lstrip('\ufeff')
        if not s or s.startswith("//"):
            continue
        if s.startswith("osu file format v"):
            try:
                bm.version = int(s.split("v")[-1])
            except ValueError:
                pass
            break

    current_section = ""
    raw_objects: List[Tuple[int, str]] = []
    combo_slots: Dict[int, Tuple[int, int, int]] = {}

    ar_specified = False
    sm_specified = False
    str_specified = False

    def _parse_rgb_triplet(v: str) -> Optional[Tuple[int, int, int]]:
        parts = [c.strip() for c in v.split(",")]
        if len(parts) != 3:
            return None
        try:
            r, g, b = (int(parts[0]), int(parts[1]), int(parts[2]))
            if 0 <= r <= 255 and 0 <= g <= 255 and 0 <= b <= 255:
                return (r, g, b)
        except ValueError:
            pass
        return None

    for line_num, raw_line in enumerate(lines, 1):
        line = raw_line.strip()
        if not line or line.startswith("//"):
            continue
        if line.startswith("[") and line.endswith("]"):
            current_section = line[1:-1]
            continue

        if current_section == "General":
            if ":" in line:
                k, v = line.split(":", 1)
                k, v = k.strip(), v.strip()
                if k == "Mode":
                    bm.mode = int(v) if v.isdigit() else 0
                elif k == "AudioFilename":
                    bm.audio_filename = v
                elif k == "AudioLeadIn":
                    try:
                        bm.audio_lead_in = int(float(v))
                    except ValueError:
                        pass
                elif k == "PreviewTime":
                    try:
                        bm.preview_time = int(float(v))
                    except ValueError:
                        pass
                elif k == "StackLeniency":
                    try:
                        bm.stack_leniency = float(v)
                    except ValueError:
                        pass

        elif current_section == "Metadata":
            if ":" in line:
                k, v = line.split(":", 1)
                k, v = k.strip(), v.strip()
                if k == "Title":
                    bm.title = v
                elif k == "TitleUnicode":
                    bm.title_unicode = v
                elif k == "Artist":
                    bm.artist = v
                elif k == "ArtistUnicode":
                    bm.artist_unicode = v
                elif k == "Creator":
                    bm.creator = v
                elif k == "Version":
                    bm.version_name = v
                elif k == "Source":
                    bm.source = v
                elif k == "Tags":
                    bm.tags = v
                elif k == "BeatmapID":
                    try:
                        bm.beatmap_id = int(v)
                    except ValueError:
                        pass
                elif k == "BeatmapSetID":
                    try:
                        bm.beatmapset_id = int(v)
                    except ValueError:
                        pass

        elif current_section == "Difficulty":
            if ":" in line:
                k, v = line.split(":", 1)
                k, v = k.strip(), v.strip()
                try:
                    val = float(v)
                    if k == "CircleSize":
                        bm.cs = val
                    elif k == "OverallDifficulty":
                        bm.od = val
                    elif k == "HPDrainRate":
                        bm.hp = val
                    elif k == "ApproachRate":
                        bm.ar = val
                        ar_specified = True
                    elif k == "SliderMultiplier":
                        bm.slider_multiplier = val
                        sm_specified = True
                    elif k == "SliderTickRate":
                        bm.slider_tick_rate = val
                        str_specified = True
                except ValueError:
                    pass

        elif current_section == "Events":
            if line.startswith("_"):
                continue
            parts = [p.strip() for p in line.split(",")]
            if not parts:
                continue
            ev_type = parts[0].strip('"').strip()
            if ev_type == "2" or ev_type.lower() == "break":
                if len(parts) == 3:
                    try:
                        st = float(parts[1].strip('"').strip())
                        et = float(parts[2].strip('"').strip())
                        if math.isfinite(st) and math.isfinite(et):
                            if st == 0.0:
                                st = 0.0
                            if et == 0.0:
                                et = 0.0
                            if et < st:
                                et = st
                            bm.breaks.append((st, et))
                    except ValueError:
                        pass

        elif current_section == "Colours":
            if ":" in line:
                k, v = line.split(":", 1)
                k, v = k.strip(), v.strip()
                if k.startswith("Combo"):
                    suffix = k[5:]
                    if suffix.isdigit():
                        idx = int(suffix)
                        if 1 <= idx <= 8:
                            rgb = _parse_rgb_triplet(v)
                            if rgb is not None:
                                combo_slots[idx] = rgb
                elif k == "SliderTrackOverride":
                    rgb = _parse_rgb_triplet(v)
                    if rgb is not None:
                        bm.slider_track_override = rgb
                elif k == "SliderBorder":
                    rgb = _parse_rgb_triplet(v)
                    if rgb is not None:
                        bm.slider_border = rgb

        elif current_section == "TimingPoints":
            parts = [p.strip() for p in line.split(",")]
            if len(parts) >= 8:
                try:
                    t = float(parts[0])
                    bl = float(parts[1])
                    meter = int(parts[2])
                    ss = int(parts[3])
                    si = int(parts[4])
                    vol = int(parts[5])
                    uninherited = (int(parts[6]) == 1)
                    eff = int(parts[7])
                    bm.timing_points.append(
                        TimingPoint(line_num, t, bl, meter, ss, si, vol, uninherited, eff)
                    )
                except ValueError:
                    pass

        elif current_section == "HitObjects":
            raw_objects.append((line_num, line))

    # Sort timing points and breaks
    bm.timing_points.sort(key=lambda tp: (tp.time, tp.line))
    bm.breaks.sort(key=lambda br: br[0])

    if combo_slots:
        bm.combo_colours = [combo_slots[idx] for idx in sorted(combo_slots)]

    # Default AR to OD if not explicitly specified (osu! specification)
    if not ar_specified:
        bm.ar = bm.od
    bm.ar_specified = ar_specified
    bm.slider_multiplier_specified = sm_specified
    bm.slider_tick_rate_specified = str_specified

    # Parse hit objects
    combo_idx = 0
    combo_num = 1
    bm.unsupported_objects = []

    for obj_idx, (line_num, line) in enumerate(raw_objects):
        parts = [p.strip() for p in line.split(",")]
        if len(parts) < 4:
            bm.unsupported_objects.append((line_num, line, "malformed_fewer_than_4_fields"))
            continue
        try:
            x = float(parts[0])
            y = float(parts[1])
            t = float(parts[2])
            flags = int(parts[3])
            hit_sound = int(parts[4]) if len(parts) > 4 else 0
        except ValueError as err:
            bm.unsupported_objects.append((line_num, line, f"malformed_field_{err}"))
            continue

        base_type = flags & (1 | 2 | 8 | 128)
        if base_type == 1:
            # Circle
            obj = HitObject(obj_idx, line_num, x, y, t, flags, hit_sound, "circle")
            obj.end_time = t
        elif base_type == 2:
            # Slider
            obj = HitObject(obj_idx, line_num, x, y, t, flags, hit_sound, "slider")
            if len(parts) > 5:
                curve_data = parts[5].split("|")
                curve_type = curve_data[0].strip() if curve_data else "L"
                controls = [(x, y)]
                for pt_str in curve_data[1:]:
                    if ":" in pt_str:
                        px, py = pt_str.split(":", 1)
                        controls.append((float(px), float(py)))
                spans = int(parts[6]) if len(parts) > 6 else 1
                length = float(parts[7]) if len(parts) > 7 else 0.0

                obj.curve_type = curve_type
                obj.controls = controls
                obj.spans = spans
                obj.declared_length = length

                # Generate geometry
                pts, cum_dist, tot_len = generate_slider_path(curve_type, controls, length)
                obj.path_points = pts
                obj.path_cum_dist = cum_dist
                obj.path_total_length = tot_len

                # Compute duration
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

                # Calculate ticks
                tick_dist = pixels_per_beat / bm.slider_tick_rate if bm.slider_tick_rate > 0 else length
                d = tick_dist
                ticks = []
                while d < length - 8.0:
                    ticks.append(d)
                    d += tick_dist
                obj.tick_distances = ticks
        elif base_type == 8:
            # Spinner
            obj = HitObject(obj_idx, line_num, x, y, t, flags, hit_sound, "spinner")
            end_t = float(parts[5]) if len(parts) > 5 else t
            obj.spinner_end_time = end_t
            obj.end_time = end_t
        else:
            # Unknown / unsupported base type
            bm.unsupported_objects.append((line_num, line, f"unsupported_base_type_{base_type}"))
            continue

        # Combo calculation
        if obj.is_new_combo or obj_idx == 0:
            combo_idx = (combo_idx + 1) % len(bm.combo_colours) if obj_idx > 0 else 0
            combo_num = 1
        obj.combo_index = combo_idx
        obj.combo_number = combo_num
        combo_num += 1

        bm.hit_objects.append(obj)

    # Sort objects by start time
    bm.hit_objects.sort(key=lambda o: (o.time, o.id))

    # Apply Stacking
    preempt, _ = bm.ar_preempt_fadein
    stack_threshold = preempt * bm.stack_leniency
    apply_stacking(bm.hit_objects, stack_threshold, bm.circle_radius)

    return bm


def apply_stacking(objects: List[HitObject], stack_threshold: float, radius: float):
    """
    Standard osu!stable stacking algorithm.
    Offsets stacked objects up and left by (radius * 0.1 * stack_height).
    """
    n = len(objects)
    stack_offset = radius * 0.1

    for i in range(n - 1, -1, -1):
        obj_i = objects[i]
        if obj_i.stack_height != 0 and obj_i.kind != "slider":
            continue

        start_time_i = obj_i.time
        pos_i = (obj_i.x, obj_i.y)

        # Check subsequent objects
        for j in range(i - 1, -1, -1):
            obj_j = objects[j]
            if start_time_i - obj_j.end_time > stack_threshold:
                break

            pos_j = (obj_j.x, obj_j.y)
            if distance(pos_i, pos_j) < 3.0:
                obj_j.stack_height = obj_i.stack_height + 1
                pos_i = pos_j
                start_time_i = obj_j.time

    # Apply stacked coordinates
    for obj in objects:
        if obj.stack_height > 0:
            offset = obj.stack_height * stack_offset
            obj.stacked_x = obj.x - offset
            obj.stacked_y = obj.y - offset
        else:
            obj.stacked_x = obj.x
            obj.stacked_y = obj.y


class ReplayFrame:
    def __init__(self, time_ms: float, x: float, y: float, keys: int, index: Optional[int] = None):
        self.time_ms = time_ms
        self.x = x
        self.y = y
        self.keys = keys
        self.index = index
        # Standard osu! replay key bits
        self.m1 = bool(keys & 1)
        self.m2 = bool(keys & 2)
        self.k1 = bool(keys & 4)
        self.k2 = bool(keys & 8)


class Replay:
    def __init__(
        self,
        player: str,
        frames: List[ReplayFrame],
        score: int = 0,
        max_combo: int = 0,
        count_300: int = 0,
        count_100: int = 0,
        count_50: int = 0,
        count_miss: int = 0,
        mode: int = 0,
        game_version: int = 0,
        beatmap_hash: str = "",
        replay_hash: str = "",
        mods: int = 0,
        perfect: int = 0,
        count_geki: int = 0,
        count_katu: int = 0,
        seed: Optional[int] = None,
        trailer_size: int = 0,
        raw_record_count: Optional[int] = None,
        source_engine: str = "python",
        normalization: Optional[Dict[str, Any]] = None,
        warnings: Optional[List[Dict[str, Any]]] = None,
    ):
        self.player = player
        self.frames = frames
        self.score = score
        self.max_combo = max_combo
        self.count_300 = count_300
        self.count_100 = count_100
        self.count_50 = count_50
        self.count_miss = count_miss
        self.mode = mode
        self.game_version = game_version
        self.beatmap_hash = beatmap_hash
        self.replay_hash = replay_hash
        self.mods = mods
        self.perfect = perfect
        self.count_geki = count_geki
        self.count_katu = count_katu
        self.seed = seed
        self.trailer_size = trailer_size
        self.raw_record_count = raw_record_count if raw_record_count is not None else len(frames)
        self.source_engine = source_engine
        self.normalization = normalization or {
            "changed_times": 0,
            "initial_dummy_removed": 0,
            "backward_removed": 0,
            "changes": [],
        }
        self.warnings = warnings or []

    def cursor_at(self, time_ms: float) -> Tuple[Optional[Tuple[float, float]], int]:
        """Interpolate replay cursor (x, y) and active keys at time_ms."""
        if not self.frames:
            return None, 0
        if time_ms <= self.frames[0].time_ms:
            f = self.frames[0]
            return (f.x, f.y), f.keys
        if time_ms >= self.frames[-1].time_ms:
            f = self.frames[-1]
            return (f.x, f.y), f.keys

        # Binary search
        low = 0
        high = len(self.frames) - 1
        while low <= high:
            mid = (low + high) // 2
            if self.frames[mid].time_ms < time_ms:
                low = mid + 1
            else:
                high = mid - 1
        idx = max(1, min(len(self.frames) - 1, low))
        f0 = self.frames[idx - 1]
        f1 = self.frames[idx]
        dt = f1.time_ms - f0.time_ms
        if dt > 1e-4:
            t = (time_ms - f0.time_ms) / dt
            x = f0.x + (f1.x - f0.x) * t
            y = f0.y + (f1.y - f0.y) * t
        else:
            x, y = f0.x, f0.y
        return (x, y), f0.keys


def parse_osr_replay(replay_path: str) -> Optional[Replay]:
    """Parse an osu! .osr replay file."""
    import struct
    import lzma

    try:
        with open(replay_path, "rb") as f:
            data = f.read()
        if len(data) < 40:
            return None
        mode = data[0]
        game_version = struct.unpack("<i", data[1:5])[0]
        offset = 5

        def read_str():
            nonlocal offset
            if offset >= len(data):
                return ""
            b = data[offset]
            offset += 1
            if b != 0x0B:
                return ""
            length, shift = 0, 0
            while True:
                byte = data[offset]
                offset += 1
                length |= (byte & 0x7F) << shift
                if not (byte & 0x80):
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
        score, max_combo = struct.unpack("<iH", data[offset : offset + 6])
        offset += 6
        perfect = data[offset]
        offset += 1
        mods = struct.unpack("<I", data[offset : offset + 4])[0]
        offset += 4
        life = read_str()
        timestamp = struct.unpack("<q", data[offset : offset + 8])[0]
        offset += 8
        lzma_len = struct.unpack("<i", data[offset : offset + 4])[0]
        offset += 4

        frames: List[ReplayFrame] = []
        seed: Optional[int] = None
        if lzma_len > 0:
            compressed = data[offset : offset + lzma_len]
            trailer_size = max(0, len(data) - (offset + lzma_len))
            raw = lzma.decompress(compressed).decode("ascii", errors="ignore")
            cur_t = 0.0
            raw_idx = 0
            trimmed = raw[:-1] if raw.endswith(",") else raw
            for p in trimmed.split(","):
                p = p.strip()
                if not p:
                    continue
                parts = p.split("|")
                if len(parts) != 4:
                    continue
                w = float(parts[0])
                px = float(parts[1])
                py = float(parts[2])
                if w == -12345.0 and px == 0.0 and py == 0.0:
                    seed = int(parts[3])
                    raw_idx += 1
                    continue
                cur_t += w
                frames.append(
                    ReplayFrame(cur_t, px, py, int(parts[3]), index=raw_idx)
                )
                raw_idx += 1
        else:
            trailer_size = max(0, len(data) - offset)

        return Replay(
            player=player,
            frames=frames,
            score=score,
            max_combo=max_combo,
            count_300=c300,
            count_100=c100,
            count_50=c50,
            count_miss=cmiss,
            mode=mode,
            game_version=game_version,
            beatmap_hash=bm_hash,
            replay_hash=rp_hash,
            mods=mods,
            perfect=perfect,
            count_geki=cgeki,
            count_katu=ckatu,
            seed=seed,
            trailer_size=trailer_size,
            raw_record_count=len(frames),
            source_engine="python",
        )
    except Exception:
        return None
