//! 밑줄의 text-decoration-skip-ink: 밑줄 띠를 지나는 글리프 잉크 둘레를 비우고 남은 조각만 긋는다.
//!
//! Blink는 띠 높이 안에서 글리프 윤곽이 차지하는 x 범위(getIntercepts)를 밑줄 두께만큼 양옆으로 넓혀 뺀다.

use super::measure::MeasuredText;
use crate::scene::Point;
use crate::text::FontBook;

/// 곡선을 펼칠 때 한 조각을 나누는 수
const CURVE_STEPS: usize = 8;

/// 기준선 아래 `top..bottom` 띠를 지나는 잉크의 x 범위(줄 왼쪽 기준)
pub fn ink_intercepts(
    fonts: &FontBook,
    text: &MeasuredText,
    font_size: f64,
    band: (f64, f64),
) -> Vec<(f64, f64)> {
    let mut ranges = Vec::new();
    for run in &text.runs {
        let line = fonts.shape_line(&run.text, font_size, run.weight);
        for glyph in &line.glyphs {
            let Some(outline) = fonts.glyph_outline(glyph, font_size) else {
                continue;
            };
            if let Some((left, right)) = band_range(&outline, band) {
                let origin = run.offset + glyph.x;
                ranges.push((origin + left, origin + right));
            }
        }
    }
    ranges
}

/// 밑줄 `0..width`에서 넓힌 잉크 범위를 뺀 조각들
pub fn underline_pieces(width: f64, intercepts: &[(f64, f64)], dilation: f64) -> Vec<(f64, f64)> {
    let mut gaps: Vec<(f64, f64)> = intercepts
        .iter()
        .map(|(left, right)| (left - dilation, right + dilation))
        .collect();
    gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut pieces = Vec::new();
    let mut start = 0.0;
    for (left, right) in gaps {
        if left > start {
            pieces.push((start, left.min(width)));
        }
        start = start.max(right);
    }
    if start < width {
        pieces.push((start, width));
    }
    pieces.retain(|(left, right)| right > left);
    pieces
}

/// 윤곽선을 짧은 선분으로 펼쳐 띠 안에 든 부분의 x 범위를 구한다
fn band_range(outline: &tiny_skia::Path, band: (f64, f64)) -> Option<(f64, f64)> {
    let mut range: Option<(f64, f64)> = None;
    for (from, to) in flattened_segments(outline) {
        for x in clipped_xs(from, to, band) {
            range = Some(match range {
                Some((left, right)) => (left.min(x), right.max(x)),
                None => (x, x),
            });
        }
    }
    range
}

fn flattened_segments(outline: &tiny_skia::Path) -> Vec<(Point, Point)> {
    use tiny_skia::PathSegment;
    let to_point = |point: tiny_skia::Point| Point::new(f64::from(point.x), f64::from(point.y));
    let mut segments = Vec::new();
    let (mut current, mut start) = (Point::new(0.0, 0.0), Point::new(0.0, 0.0));
    for segment in outline.segments() {
        let samples: Vec<Point> = match segment {
            PathSegment::MoveTo(point) => {
                current = to_point(point);
                start = current;
                continue;
            }
            PathSegment::LineTo(point) => vec![to_point(point)],
            PathSegment::QuadTo(control, point) => {
                sample_curve(&[current, to_point(control), to_point(point)])
            }
            PathSegment::CubicTo(first, second, point) => {
                sample_curve(&[current, to_point(first), to_point(second), to_point(point)])
            }
            PathSegment::Close => vec![start],
        };
        for point in samples {
            segments.push((current, point));
            current = point;
        }
    }
    segments
}

/// 베지어(제어점 3·4개)를 de Casteljau로 나눈 점들(시작점 제외)
fn sample_curve(controls: &[Point]) -> Vec<Point> {
    (1..=CURVE_STEPS)
        .map(|step| {
            let t = step as f64 / CURVE_STEPS as f64;
            let mut points = controls.to_vec();
            while points.len() > 1 {
                points = points
                    .windows(2)
                    .map(|pair| {
                        Point::new(
                            pair[0].x + (pair[1].x - pair[0].x) * t,
                            pair[0].y + (pair[1].y - pair[0].y) * t,
                        )
                    })
                    .collect();
            }
            points[0]
        })
        .collect()
}

/// 선분이 띠 안에 남기는 x(끝점 또는 띠 경계와의 교점)
fn clipped_xs(from: Point, to: Point, (top, bottom): (f64, f64)) -> Vec<f64> {
    let is_inside = |point: Point| point.y >= top && point.y <= bottom;
    let mut xs: Vec<f64> = [from, to]
        .into_iter()
        .filter(|point| is_inside(*point))
        .map(|point| point.x)
        .collect();
    for edge in [top, bottom] {
        let crosses = (from.y - edge) * (to.y - edge) < 0.0;
        if crosses {
            let t = (edge - from.y) / (to.y - from.y);
            xs.push(from.x + (to.x - from.x) * t);
        }
    }
    xs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 잉크_범위를_넓혀_밑줄을_끊는다() {
        let pieces = underline_pieces(100.0, &[(40.0, 45.0)], 1.0);
        assert_eq!(pieces, vec![(0.0, 39.0), (46.0, 100.0)]);
        assert_eq!(underline_pieces(10.0, &[], 1.0), vec![(0.0, 10.0)]);
    }
}
