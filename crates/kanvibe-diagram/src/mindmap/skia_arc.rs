//! Chromium이 원호(`a`) 경로의 getBBox를 구하는 계산을 f32로 그대로 따른다.
//!
//! Blink는 SVG 원호를 Skia `SkPath::arcTo`로 120° 이하 원뿔 곡선(conic) 조각으로 바꾸고, `computeTightBounds`로
//! 조각마다 극값을 찾아 범위를 잡는다. cose-bilkent 배치는 노드 크기의 마지막 자리에도 결과가 크게 달라지므로
//! 구름·폭발 모양 노드 크기는 이 f32 연산 순서까지 맞춰야 기준 SVG와 같은 배치가 나온다.

/// 상대 좌표 원호 하나: (rx, ry, x축 회전(도), 큰 호, 쓸기 방향, dx, dy)
pub(crate) type RelativeArc = (f64, f64, f64, bool, bool, f64, f64);

/// `SK_ScalarPI`
const PI: f32 = std::f32::consts::PI;
/// `SK_ScalarNearlyZero`(1/4096). 회전 행렬의 아주 작은 sin·cos는 0으로 맞춘다
const NEARLY_ZERO: f32 = 1.0 / 4096.0;

/// 경로 범위 (min_x, min_y, max_x, max_y)
pub(crate) type F32Bounds = (f32, f32, f32, f32);

/// sinf·cosf·tanf·atan2f를 배정밀도로 계산해 f32로 반올림한다(glibc f32 함수와 같은 정확히 반올림된 값)
fn sin(value: f32) -> f32 {
    f64::from(value).sin() as f32
}

fn cos(value: f32) -> f32 {
    f64::from(value).cos() as f32
}

fn atan2(y: f32, x: f32) -> f32 {
    f64::from(y).atan2(f64::from(x)) as f32
}

fn snap_to_zero(value: f32) -> f32 {
    if value.abs() <= NEARLY_ZERO {
        0.0
    } else {
        value
    }
}

/// SkMatrix(원근 없는 아핀 행렬)
#[derive(Clone, Copy, Debug)]
struct SkMatrix {
    scale_x: f32,
    skew_x: f32,
    trans_x: f32,
    skew_y: f32,
    scale_y: f32,
    trans_y: f32,
}

impl SkMatrix {
    fn scale(scale_x: f32, scale_y: f32) -> Self {
        Self {
            scale_x,
            skew_x: 0.0,
            trans_x: 0.0,
            skew_y: 0.0,
            scale_y,
            trans_y: 0.0,
        }
    }

    /// setRotate: SkDegreesToRadians 뒤 sin·cos를 0 근처에서 자른다
    fn rotate(degrees: f32) -> Self {
        let radians = degrees * (PI / 180.0);
        let (sine, cosine) = (snap_to_zero(sin(radians)), snap_to_zero(cos(radians)));
        Self {
            scale_x: cosine,
            skew_x: -sine,
            trans_x: 0.0,
            skew_y: sine,
            scale_y: cosine,
            trans_y: 0.0,
        }
    }

    /// setConcat(self, other). 곱의 합은 배정밀도로 계산한다(`muladdmul`)
    fn concat(&self, other: &SkMatrix) -> Self {
        let mul_add = |a: f32, b: f32, c: f32, d: f32| {
            (f64::from(a) * f64::from(b) + f64::from(c) * f64::from(d)) as f32
        };
        Self {
            scale_x: mul_add(self.scale_x, other.scale_x, self.skew_x, other.skew_y),
            skew_x: mul_add(self.scale_x, other.skew_x, self.skew_x, other.scale_y),
            trans_x: mul_add(self.scale_x, other.trans_x, self.skew_x, other.trans_y)
                + self.trans_x,
            skew_y: mul_add(self.skew_y, other.scale_x, self.scale_y, other.skew_y),
            scale_y: mul_add(self.skew_y, other.skew_x, self.scale_y, other.scale_y),
            trans_y: mul_add(self.skew_y, other.trans_x, self.scale_y, other.trans_y)
                + self.trans_y,
        }
    }

    /// preScale: 열마다 배율을 곱한다
    fn pre_scale(&self, scale_x: f32, scale_y: f32) -> Self {
        Self {
            scale_x: self.scale_x * scale_x,
            skew_x: self.skew_x * scale_y,
            skew_y: self.skew_y * scale_x,
            scale_y: self.scale_y * scale_y,
            ..*self
        }
    }

    fn map(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (
            x * self.scale_x + y * self.skew_x + self.trans_x,
            x * self.skew_y + y * self.scale_y + self.trans_y,
        )
    }
}

/// 원뿔 곡선 조각: 조절점, 끝점, 무게
type Conic = ((f32, f32), (f32, f32), f32);

/// `M0 0` 뒤 상대 원호들과 `H0 V0 Z`로 된 경로의 computeTightBounds
pub(crate) fn arc_path_bounds(arcs: &[RelativeArc]) -> F32Bounds {
    let mut bounds: F32Bounds = (0.0, 0.0, 0.0, 0.0);
    let mut include = |(x, y): (f32, f32)| {
        bounds = (
            bounds.0.min(x),
            bounds.1.min(y),
            bounds.2.max(x),
            bounds.3.max(y),
        );
    };
    let mut current = (0.0f32, 0.0f32);
    for &(radius_x, radius_y, rotation, is_large, is_sweep, dx, dy) in arcs {
        let target = (current.0 + dx as f32, current.1 + dy as f32);
        let arc = ArcRequest {
            radius_x: radius_x as f32,
            radius_y: radius_y as f32,
            rotation: rotation as f32,
            is_large,
            is_sweep,
        };
        let mut start = current;
        for (control, end, weight) in arc.to_conics(current, target) {
            conic_extrema(start, control, end, weight)
                .into_iter()
                .for_each(&mut include);
            include(end);
            start = end;
        }
        include(target);
        current = target;
    }
    include((0.0, current.1));
    include((0.0, 0.0));
    bounds
}

struct ArcRequest {
    radius_x: f32,
    radius_y: f32,
    rotation: f32,
    is_large: bool,
    is_sweep: bool,
}

impl ArcRequest {
    /// SkPath::arcTo(rx, ry, angle, arcLarge, sweep, x, y). 비어 있으면 곧은 선이다
    fn to_conics(&self, start: (f32, f32), end: (f32, f32)) -> Vec<Conic> {
        let (mut rx, mut ry) = (self.radius_x.abs(), self.radius_y.abs());
        if rx == 0.0 || ry == 0.0 || start == end {
            return Vec::new();
        }
        let half = ((start.0 - end.0) * 0.5, (start.1 - end.1) * 0.5);
        let (mid_x, mid_y) = SkMatrix::rotate(-self.rotation).map(half);
        let radii_scale = (mid_x * mid_x) / (rx * rx) + (mid_y * mid_y) / (ry * ry);
        if radii_scale > 1.0 {
            let factor = radii_scale.sqrt();
            rx *= factor;
            ry *= factor;
        }
        let to_unit = SkMatrix::scale(1.0 / rx, 1.0 / ry).concat(&SkMatrix::rotate(-self.rotation));
        let (unit_start, unit_end) = (to_unit.map(start), to_unit.map(end));
        let center = self.unit_center(unit_start, unit_end);
        let theta_start = atan2(unit_start.1 - center.1, unit_start.0 - center.0);
        let theta_end = atan2(unit_end.1 - center.1, unit_end.0 - center.0);
        let mut theta_arc = theta_end - theta_start;
        if theta_arc < 0.0 && self.is_sweep {
            theta_arc += PI * 2.0;
        } else if theta_arc > 0.0 && !self.is_sweep {
            theta_arc -= PI * 2.0;
        }
        if theta_arc.abs() < PI / (1000.0 * 1000.0) {
            return Vec::new();
        }
        let from_unit = SkMatrix::rotate(self.rotation).pre_scale(rx, ry);
        let mut conics = unit_conics(theta_start, theta_arc, center, &from_unit);
        if let Some(last) = conics.last_mut() {
            last.1 = end;
        }
        conics
    }

    /// 단위 원으로 옮긴 두 끝점에서 원 중심을 구한다
    fn unit_center(&self, unit_start: (f32, f32), unit_end: (f32, f32)) -> (f32, f32) {
        let delta = (unit_end.0 - unit_start.0, unit_end.1 - unit_start.1);
        let distance = delta.0 * delta.0 + delta.1 * delta.1;
        let mut scale_factor = (1.0 / distance - 0.25).max(0.0).sqrt();
        // SkPathDirection::kCCW는 쓸기 방향 0이다
        if !self.is_sweep != self.is_large {
            scale_factor = -scale_factor;
        }
        let (dx, dy) = (delta.0 * scale_factor, delta.1 * scale_factor);
        let middle = (
            (unit_start.0 + unit_end.0) * 0.5,
            (unit_start.1 + unit_end.1) * 0.5,
        );
        (middle.0 - dy, middle.1 + dx)
    }
}

/// 호를 120° 이하 조각으로 나눠 조각마다 끝점·조절점·무게를 만든다
fn unit_conics(
    theta_start: f32,
    theta_arc: f32,
    center: (f32, f32),
    from_unit: &SkMatrix,
) -> Vec<Conic> {
    let segments = (theta_arc / (2.0 * PI / 3.0)).abs().ceil() as usize;
    let theta_width = theta_arc / segments as f32;
    let tangent = (f64::from(0.5 * theta_width).tan()) as f32;
    let weight = (0.5 + cos(theta_width) * 0.5).sqrt();
    let mut start_theta = theta_start;
    (0..segments)
        .map(|_| {
            let end_theta = start_theta + theta_width;
            let (sine, cosine) = (snap_to_zero(sin(end_theta)), snap_to_zero(cos(end_theta)));
            let end = (cosine + center.0, sine + center.1);
            let control = (end.0 + tangent * sine, end.1 + -(tangent * cosine));
            start_theta = end_theta;
            (from_unit.map(control), from_unit.map(end), weight)
        })
        .collect()
}

/// compute_conic_extremas: x·y 방향 극값 위치에서 원뿔 곡선 값을 구한다
fn conic_extrema(
    start: (f32, f32),
    control: (f32, f32),
    end: (f32, f32),
    weight: f32,
) -> Vec<(f32, f32)> {
    let mut roots = axis_extrema(start.0, control.0, end.0, weight);
    roots.extend(axis_extrema(start.1, control.1, end.1, weight));
    roots
        .into_iter()
        .map(|t| {
            (
                conic_at(start.0, control.0, end.0, weight, t),
                conic_at(start.1, control.1, end.1, weight, t),
            )
        })
        .collect()
}

fn axis_extrema(p0: f32, p1: f32, p2: f32, weight: f32) -> Vec<f32> {
    let p20 = p2 - p0;
    let p10 = p1 - p0;
    let weighted_p10 = weight * p10;
    unit_quad_roots(weight * p20 - p20, p20 - 2.0 * weighted_p10, weighted_p10)
}

/// SkConicCoeff 분자·분모 2차식을 계산해 나눈다
fn conic_at(p0: f32, p1: f32, p2: f32, weight: f32, t: f32) -> f32 {
    let weighted_p1 = p1 * weight;
    let numerator_a = p2 - 2.0 * weighted_p1 + p0;
    let numerator_b = 2.0 * (weighted_p1 - p0);
    let denominator_b = 2.0 * (weight - 1.0);
    let denominator_a = 0.0 - denominator_b;
    let numerator = (numerator_a * t + numerator_b) * t + p0;
    let denominator = (denominator_a * t + denominator_b) * t + 1.0;
    numerator / denominator
}

/// SkFindUnitQuadRoots: 0과 1 사이(양 끝 제외) 근을 오름차순으로
fn unit_quad_roots(a: f32, b: f32, c: f32) -> Vec<f32> {
    if a == 0.0 {
        return unit_divide(-c, b).into_iter().collect();
    }
    let discriminant = f64::from(b) * f64::from(b) - 4.0 * f64::from(a) * f64::from(c);
    if discriminant < 0.0 {
        return Vec::new();
    }
    let root = discriminant.sqrt() as f32;
    if !root.is_finite() {
        return Vec::new();
    }
    let q = if b < 0.0 {
        -(b - root) / 2.0
    } else {
        -(b + root) / 2.0
    };
    let mut roots: Vec<f32> = unit_divide(q, a)
        .into_iter()
        .chain(unit_divide(c, q))
        .collect();
    if roots.len() == 2 {
        if roots[0] > roots[1] {
            roots.swap(0, 1);
        } else if roots[0] == roots[1] {
            roots.pop();
        }
    }
    roots
}

fn unit_divide(numerator: f32, denominator: f32) -> Option<f32> {
    let (numerator, denominator) = if numerator < 0.0 {
        (-numerator, -denominator)
    } else {
        (numerator, denominator)
    };
    if denominator == 0.0 || numerator == 0.0 || numerator >= denominator {
        return None;
    }
    let ratio = numerator / denominator;
    (!ratio.is_nan() && ratio != 0.0).then_some(ratio)
}
