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

/// Обычная timing point (uninherited = 1); beat_length задана в мс на бит.
#[derive(Debug, PartialEq)]
pub struct TimingPoint {
    pub line: usize,
    pub time: f64,
    pub beat_length: f64,
    pub meter: u32,
    pub sample_set: u8,
    pub sample_index: u32,
    pub volume: u8,
    pub effects: u32,
}
