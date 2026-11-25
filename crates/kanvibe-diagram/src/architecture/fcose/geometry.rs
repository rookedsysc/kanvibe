//! layout-base 2.0 `RectangleD`·`IGeometry`·`IMath`에서 힘 계산이 쓰는 부분.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn set_center(&mut self, center_x: f64, center_y: f64) {
        self.x = center_x - self.width / 2.0;
        self.y = center_y - self.height / 2.0;
    }

    /// 모서리가 닿기만 해도 겹친다고 본다
    pub fn intersects(&self, other: &Rect) -> bool {
        !(self.right() < other.x
            || self.bottom() < other.y
            || other.right() < self.x
            || other.bottom() < self.y)
    }
}

/// IMath.sign
pub(crate) fn sign(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// IGeometry.calcSeparationAmount: 겹친 두 사각형을 떼어 놓을 x·y 이동량의 절반(여유 포함)
pub(crate) fn separation_amount(a: &Rect, b: &Rect, buffer: f64) -> (f64, f64) {
    let direction_x = if a.center_x() < b.center_x() {
        -1.0
    } else {
        1.0
    };
    let direction_y = if a.center_y() < b.center_y() {
        -1.0
    } else {
        1.0
    };
    let mut overlap_x = a.right().min(b.right()) - a.x.max(b.x);
    let mut overlap_y = a.bottom().min(b.bottom()) - a.y.max(b.y);
    if a.x <= b.x && a.right() >= b.right() {
        overlap_x += (b.x - a.x).min(a.right() - b.right());
    } else if b.x <= a.x && b.right() >= a.right() {
        overlap_x += (a.x - b.x).min(b.right() - a.right());
    }
    if a.y <= b.y && a.bottom() >= b.bottom() {
        overlap_y += (b.y - a.y).min(a.bottom() - b.bottom());
    } else if b.y <= a.y && b.bottom() >= a.bottom() {
        overlap_y += (a.y - b.y).min(b.bottom() - a.bottom());
    }
    let mut slope = ((b.center_y() - a.center_y()) / (b.center_x() - a.center_x())).abs();
    if b.center_y() == a.center_y() && b.center_x() == a.center_x() {
        slope = 1.0;
    }
    let mut move_y = slope * overlap_x;
    let mut move_x = overlap_y / slope;
    if overlap_x < move_x {
        move_x = overlap_x;
    } else {
        move_y = overlap_y;
    }
    (
        -direction_x * (move_x / 2.0 + buffer),
        -direction_y * (move_y / 2.0 + buffer),
    )
}

/// 두 중심을 잇는 선이 각 사각형 테두리와 만나는 점. 겹치면 `None`(IGeometry.getIntersection2)
pub(crate) fn clip_points(a: &Rect, b: &Rect) -> Option<[f64; 4]> {
    if a.intersects(b) {
        return None;
    }
    let (p1x, p1y, p2x, p2y) = (a.center_x(), a.center_y(), b.center_x(), b.center_y());
    if p1x == p2x {
        return Some(if p1y > p2y {
            [p1x, a.y, p2x, b.bottom()]
        } else {
            [p1x, a.bottom(), p2x, b.y]
        });
    }
    if p1y == p2y {
        return Some(if p1x > p2x {
            [a.x, p1y, b.right(), p2y]
        } else {
            [a.right(), p1y, b.x, p2y]
        });
    }
    Some(slanted_clip_points(a, b))
}

/// 꼭짓점 판정은 원본처럼 기울기를 정확히 비교한다
fn corner_clip(
    rect: &Rect,
    slope: f64,
    slope_prime: f64,
    is_toward_left: bool,
) -> Option<(f64, f64)> {
    if -slope == slope_prime {
        return Some(if is_toward_left {
            (rect.x, rect.bottom())
        } else {
            (rect.right(), rect.y)
        });
    }
    if slope == slope_prime {
        return Some(if is_toward_left {
            (rect.x, rect.y)
        } else {
            (rect.right(), rect.bottom())
        });
    }
    None
}

fn slanted_clip_points(a: &Rect, b: &Rect) -> [f64; 4] {
    let (p1x, p1y, p2x, p2y) = (a.center_x(), a.center_y(), b.center_x(), b.center_y());
    let slope_a = a.height / a.width;
    let slope_b = b.height / b.width;
    let slope_prime = (p2y - p1y) / (p2x - p1x);
    let corner_a = corner_clip(a, slope_a, slope_prime, p1x > p2x);
    let corner_b = corner_clip(b, slope_b, slope_prime, p2x > p1x);
    if let (Some(first), Some(second)) = (corner_a, corner_b) {
        return [first.0, first.1, second.0, second.1];
    }
    let (direction_a, direction_b) = match (p1x > p2x, p1y > p2y) {
        (true, true) => (
            cardinal(slope_a, slope_prime, 4),
            cardinal(slope_b, slope_prime, 2),
        ),
        (true, false) => (
            cardinal(-slope_a, slope_prime, 3),
            cardinal(-slope_b, slope_prime, 1),
        ),
        (false, true) => (
            cardinal(-slope_a, slope_prime, 1),
            cardinal(-slope_b, slope_prime, 3),
        ),
        (false, false) => (
            cardinal(slope_a, slope_prime, 2),
            cardinal(slope_b, slope_prime, 4),
        ),
    };
    let (ax, ay) = corner_a.unwrap_or_else(|| side_clip(a, direction_a, slope_prime));
    let (bx, by) = corner_b.unwrap_or_else(|| side_clip(b, direction_b, slope_prime));
    [ax, ay, bx, by]
}

/// IGeometry.getCardinalDirection: 1 북, 2 동, 3 남, 4 서
fn cardinal(slope: f64, slope_prime: f64, line: u8) -> u8 {
    if slope > slope_prime {
        line
    } else {
        1 + line % 4
    }
}

fn side_clip(rect: &Rect, direction: u8, slope_prime: f64) -> (f64, f64) {
    let (center_x, center_y) = (rect.center_x(), rect.center_y());
    let half_width = rect.width / 2.0;
    let half_height = rect.height / 2.0;
    match direction {
        1 => (center_x + (-half_height) / slope_prime, rect.y),
        2 => (rect.right(), center_y + half_width * slope_prime),
        3 => (center_x + half_height / slope_prime, rect.bottom()),
        _ => (rect.x, center_y + (-half_width) * slope_prime),
    }
}
