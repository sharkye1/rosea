//! Разобранные данные карты с исходными позициями для диагностики.

/// Индекс записи HitObjects до сортировки; начинается с нуля.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ObjectId(pub u32);

/// Координаты в osu!pixels; положение за границами поля допустимо.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, PartialEq)]
pub struct ReplayRecord {
    pub index: u32,
    pub delta_ms: i64,
    pub cursor: Point,
    pub keys: u32,
}

#[derive(Debug, PartialEq)]
pub struct ReplayInput {
    pub(crate) records: Vec<ReplayRecord>,
    pub(crate) seed: Option<i32>,
}

impl ReplayInput {
    pub fn records(&self) -> &[ReplayRecord] {
        &self.records
    }
    pub fn seed(&self) -> Option<i32> {
        self.seed
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReplayMeta {
    pub mode: u8,
    pub(crate) game_version: i32,
    pub(crate) beatmap_hash: String,
    pub player: String,
    pub replay_hash: String,
    pub count_geki: u16,
    pub count_katu: u16,
    pub perfect: u8,
    pub mods: u32,
    pub life_graph: String,
    pub timestamp: [u8; 8],
    pub trailer: Vec<u8>,
}

impl ReplayMeta {
    pub fn game_version(&self) -> i32 {
        self.game_version
    }
    pub fn beatmap_hash(&self) -> &str {
        &self.beatmap_hash
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ExpectedStats {
    pub score: i32,
    pub max_combo: u16,
    pub count_300: u16,
    pub count_100: u16,
    pub count_50: u16,
    pub count_miss: u16,
}

#[derive(Debug, PartialEq)]
pub struct ParsedReplay {
    pub meta: ReplayMeta,
    pub expected: ExpectedStats,
    pub input: ReplayInput,
}

#[derive(Debug, PartialEq)]
pub struct Parsed<T> {
    pub value: T,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, PartialEq)]
pub enum Warning {
    ReplayTrailerNotInterpreted,
    ReplayInputNormalized {
        changed_times: u32,
        initial_dummy_removed: u32,
        backward_removed: u32,
        changes: Vec<ReplayChange>,
    },
}

#[derive(Debug, PartialEq)]
pub enum ReplayChange {
    Time {
        index: u32,
        old_ms: i64,
        new_ms: i64,
    },
    InitialDummy {
        index: u32,
    },
    BackwardFrame {
        index: u32,
    },
}

/// Круг до подготовки геометрии: время в мс, line — строка исходника с единицы.
#[derive(Debug, PartialEq)]
pub struct Circle {
    pub id: ObjectId,
    pub line: usize,
    pub position: Point,
    pub start_time: f64,
    pub flags: u8,
    pub hit_sound: u8,
}

/// Поддерживаемые типы пути: ломаная L, кривая Безье B и дуга окружности P.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CurveKind {
    Linear,
    Bezier,
    PerfectCurve,
}

#[derive(Debug, PartialEq)]
pub enum HitObjectKind {
    Circle,
    Slider {
        kind: CurveKind,
        // Исходные мировые координаты, включая голову слайдера первой точкой.
        controls: Vec<Point>,
        // Число полных проходов; разворотов на один меньше.
        spans: u32,
        declared_length: f64,
    },
    Spinner {
        // Время окончания спиннера в мс.
        end: f64,
    },
}

/// Общие поля круга, слайдера и спиннера до подготовки геометрии и времён.
#[derive(Debug, PartialEq)]
pub struct HitObject {
    pub id: ObjectId,
    pub line: usize,
    pub position: Point,
    pub start_time: f64,
    pub flags: u8,
    pub hit_sound: u8,
    pub kind: HitObjectKind,
}

/// Исходная timing point. Смысл beat_length зависит от флага uninherited:
/// true — положительная длительность бита в мс;
/// false — обратный множитель скорости в процентах; неотрицательное значение сбрасывает скорость.
#[derive(Debug, PartialEq)]
pub struct TimingPoint {
    pub line: usize,
    pub time: f64,
    pub beat_length: f64,
    pub meter: u32,
    pub sample_set: u8,
    pub sample_index: i32,
    pub volume: u8,
    pub uninherited: bool,
    pub effects: u32,
}

/// Интервал перерыва между объектами в мс.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BreakPeriod {
    pub start_time: f64,
    pub end_time: f64,
}

/// Метаданные карты из секции `[Metadata]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub title_unicode: Option<String>,
    pub artist: String,
    pub artist_unicode: Option<String>,
    pub creator: String,
    pub version: String,
    pub source: String,
    pub tags: String,
    pub beatmap_id: Option<i32>,
    pub beatmap_set_id: Option<i32>,
}

impl Metadata {
    /// Возвращает Юникод-вариант названия песни, если он задан и не пуст, иначе романизированное `title`.
    pub fn title_unicode_or_title(&self) -> &str {
        match &self.title_unicode {
            Some(u) if !u.is_empty() => u,
            _ => &self.title,
        }
    }

    /// Возвращает Юникод-вариант имени исполнителя, если он задан и не пуст, иначе романизированное `artist`.
    pub fn artist_unicode_or_artist(&self) -> &str {
        match &self.artist_unicode {
            Some(u) if !u.is_empty() => u,
            _ => &self.artist,
        }
    }
}

/// Цвет в формате RGB (0..=255 на канал).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Упаковывает цвет в формат `0x00RRGGBB` для кадрового буфера.
    pub const fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }
}

/// Четыре стандартных цвета комбо, используемых по умолчанию при отсутствии секции `[Colours]`.
pub const DEFAULT_COMBO_COLOURS: [Rgb; 4] = [
    Rgb::new(255, 192, 0),
    Rgb::new(0, 202, 0),
    Rgb::new(18, 124, 255),
    Rgb::new(242, 24, 57),
];

/// Цветовая информация карты из секции `[Colours]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colours {
    /// Цвета комбо (Combo1..Combo8 по порядку номеров; при отсутствии используются 4 стандартных цвета).
    pub combo: Vec<Rgb>,
    /// Переопределение цвета дорожки слайдера.
    pub slider_track_override: Option<Rgb>,
    /// Переопределение цвета границы слайдера.
    pub slider_border: Option<Rgb>,
}

impl Default for Colours {
    fn default() -> Self {
        Self {
            combo: DEFAULT_COMBO_COLOURS.to_vec(),
            slider_track_override: None,
            slider_border: None,
        }
    }
}
