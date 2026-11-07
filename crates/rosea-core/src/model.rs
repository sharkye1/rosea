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
    pub sample_index: u32,
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
