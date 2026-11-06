#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ObjectId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, PartialEq)]
pub struct Circle {
    pub id: ObjectId,
    pub line: usize,
    pub position: Point,
    pub start_time: f64,
    pub flags: u8,
    pub hit_sound: u8,
}
