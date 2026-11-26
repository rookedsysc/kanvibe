//! FreeType autofit(light) 세로 맞춤 흉내.
//!
//! 기준 이미지의 Chromium은 hintslight로 글자를 그린다. autofit은 x-height 오버슈트가 정수 픽셀이 되도록 세로
//! 배율을 정한 뒤(`af_latin_metrics_scale_dim`), 기준선·x-height·대문자·어센더·디센더 청색 영역을 각각 픽셀에
//! 반올림해 맞추고 그 사이 점들을 보간한다. 여기서는 Noto Sans의 청색 영역 높이로 만든 구간별 직선 사상으로
//! 글리프 점의 세로 위치만 옮긴다. 가로 위치는 부분 픽셀 그대로다.

use tiny_skia::{Path, PathBuilder, PathSegment, Point as SkiaPoint};

/// x-height 오버슈트를 픽셀에 맞출 때 올림으로 치는 소수부 경계(`(scaled + 40) & ~63`)
const ROUND_UP_BIAS: f64 = 40.0 / 64.0;
/// Noto Sans 청색 영역(글꼴 단위 1000 기준): x-height 평평한 높이와 오버슈트
const X_HEIGHT_FLAT: f64 = 0.536;
const X_HEIGHT_SHOOT: f64 = 0.546;
/// 대문자 평평한 높이('H')와 오버슈트('O'), 어센더('l'), 디센더('p')
const CAPITAL_FLAT: f64 = 0.714;
const CAPITAL_SHOOT: f64 = 0.725;
const ASCENDER: f64 = 0.760;
const DESCENDER: f64 = -0.240;
/// 기준선 아래 오버슈트('o' 바닥)
const BASELINE_SHOOT: f64 = -0.010;

/// 장치 픽셀 글자 크기 하나에 대한 세로 사상. 값은 기준선 위쪽을 양수로 잰 장치 픽셀이다
#[derive(Clone, Debug)]
pub struct VerticalHinting {
    /// (힌팅 전 높이, 힌팅 뒤 높이), 높이 순
    stops: Vec<(f64, f64)>,
    /// 마지막 구간 밖에서 쓰는 기울기(autofit 세로 배율)
    scale: f64,
}

impl VerticalHinting {
    /// `device_size`는 배율까지 곱한 글자 크기(px)다. 1px보다 작은 x-height는 맞추지 않는다
    pub fn for_size(device_size: f64) -> Self {
        let shoot = X_HEIGHT_SHOOT * device_size;
        if shoot < 1.0 {
            return Self {
                stops: vec![(0.0, 0.0)],
                scale: 1.0,
            };
        }
        let scale = (shoot + ROUND_UP_BIAS).floor() / shoot;
        let fitted = |height: f64| (height * device_size * scale).round();
        let x_height = fitted(X_HEIGHT_FLAT);
        let capital = fitted(CAPITAL_FLAT);
        let stops = vec![
            (DESCENDER * device_size, fitted(DESCENDER)),
            (BASELINE_SHOOT * device_size, 0.0),
            (0.0, 0.0),
            (X_HEIGHT_FLAT * device_size, x_height),
            (X_HEIGHT_SHOOT * device_size, x_height),
            (CAPITAL_FLAT * device_size, capital),
            (CAPITAL_SHOOT * device_size, capital),
            (ASCENDER * device_size, fitted(ASCENDER).max(capital)),
        ];
        Self { stops, scale }
    }

    /// 기준선 위 높이 하나를 옮긴다
    pub fn map(&self, height: f64) -> f64 {
        let (first, last) = (self.stops[0], self.stops[self.stops.len() - 1]);
        if height <= first.0 {
            return first.1 + (height - first.0) * self.scale;
        }
        if height >= last.0 {
            return last.1 + (height - last.0) * self.scale;
        }
        let upper = self
            .stops
            .iter()
            .position(|stop| stop.0 >= height)
            .unwrap_or(self.stops.len() - 1);
        let (low, high) = (self.stops[upper - 1], self.stops[upper]);
        let ratio = (height - low.0) / (high.0 - low.0);
        low.1 + (high.1 - low.1) * ratio
    }

    /// 원점 기준 장치 픽셀 윤곽선(y는 아래로 증가)의 세로 위치를 옮긴 새 윤곽선
    pub fn apply(&self, outline: &Path) -> Option<Path> {
        let shift =
            |point: SkiaPoint| SkiaPoint::from_xy(point.x, -self.map(-f64::from(point.y)) as f32);
        let mut builder = PathBuilder::new();
        for segment in outline.segments() {
            match segment {
                PathSegment::MoveTo(point) => builder.move_to(shift(point).x, shift(point).y),
                PathSegment::LineTo(point) => builder.line_to(shift(point).x, shift(point).y),
                PathSegment::QuadTo(control, point) => {
                    let (control, point) = (shift(control), shift(point));
                    builder.quad_to(control.x, control.y, point.x, point.y);
                }
                PathSegment::CubicTo(first, second, point) => {
                    let (first, second, point) = (shift(first), shift(second), shift(point));
                    builder.cubic_to(first.x, first.y, second.x, second.y, point.x, point.y);
                }
                PathSegment::Close => builder.close(),
            }
        }
        builder.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 어센더는_x_height_배율과_따로_픽셀에_맞춘다() {
        // fc-korean-lr--desktop(배율 0.6724)의 'l'은 8px 높이로 그려진다
        let hinting = VerticalHinting::for_size(16.0 * 0.6724);
        assert_eq!(hinting.map(ASCENDER * 16.0 * 0.6724), 8.0);
        assert_eq!(hinting.map(X_HEIGHT_SHOOT * 16.0 * 0.6724), 6.0);
        assert_eq!(hinting.map(0.0), 0.0);
    }
}
