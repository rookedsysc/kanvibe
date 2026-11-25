//! treemap이 쓰는 테마 색(`cScale0..11`·`cScalePeer0..11`·`cScaleLabel0..11`)과 d3 `scaleOrdinal`.
//! 기본 테마의 값은 theme-default.js `updateColors`가 두 번 불려 `cScale`만 두 번 어두워진 결과이고,
//! dark 테마의 `cScalePeer`는 `cScale`을 10% 밝힌 색이다. 값은 기준 SVG와 맞췄다.

use crate::scene::Color;
use crate::theme::{DARK_SCALE, LIGHT_SCALE_HUES, Theme, hsl, lighten};

const SCALE_SIZE: usize = 12;

#[derive(Clone, Debug)]
pub struct TreemapColors {
    fills: [Color; SCALE_SIZE],
    peers: [Color; SCALE_SIZE],
    labels: [Color; SCALE_SIZE],
    pub title: Color,
}

impl TreemapColors {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => light(),
            Theme::Dark => dark(),
        }
    }

    /// `["transparent", cScale0, …]` 순서 척도에서 `index`번째 색
    pub fn fill(&self, index: usize) -> Color {
        match index % (SCALE_SIZE + 1) {
            0 => Color::rgba(0, 0, 0, 0.0),
            slot => self.fills[slot - 1],
        }
    }

    pub fn peer(&self, index: usize) -> Color {
        match index % (SCALE_SIZE + 1) {
            0 => Color::rgba(0, 0, 0, 0.0),
            slot => self.peers[slot - 1],
        }
    }

    pub fn label(&self, index: usize) -> Color {
        self.labels[index % SCALE_SIZE]
    }
}

/// d3 `scaleOrdinal`: 처음 물어본 순서대로 이름에 번호를 준다
#[derive(Clone, Debug, Default)]
pub struct OrdinalScale {
    domain: Vec<String>,
}

impl OrdinalScale {
    pub fn index(&mut self, name: &str) -> usize {
        if let Some(index) = self.domain.iter().position(|known| known == name) {
            return index;
        }
        self.domain.push(name.to_string());
        self.domain.len() - 1
    }
}

fn light() -> TreemapColors {
    let hues = LIGHT_SCALE_HUES;
    let white = Color::rgb(255, 255, 255);
    let black = Color::rgb(0, 0, 0);
    TreemapColors {
        fills: std::array::from_fn(|index| {
            let lightness = if index == 1 {
                73.529_411_764_7
            } else {
                76.274_509_803_9
            };
            hsl(hues[index], 100.0, lightness)
        }),
        // cScalePeer1·2는 secondary·tertiary를 45%·40% 어둡게 한 색이고 나머지는 첫 cScale을 25% 어둡게 한 색이다
        peers: std::array::from_fn(|index| {
            let lightness = match index {
                1 => 48.529_411_764_7,
                2 => 56.274_509_803_9,
                _ => 61.274_509_803_9,
            };
            hsl(hues[index], 100.0, lightness)
        }),
        labels: std::array::from_fn(|index| {
            if index == 0 || index == 3 {
                white
            } else {
                black
            }
        }),
        title: Color::rgb(0x33, 0x33, 0x33),
    }
}

fn dark() -> TreemapColors {
    TreemapColors {
        fills: DARK_SCALE,
        peers: DARK_SCALE.map(|color| lighten(color, 10.0)),
        labels: [Color::rgb(211, 211, 211); SCALE_SIZE],
        title: Color::rgb(0xF9, 0xFF, 0xFE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_테마_테두리는_바탕을_10퍼센트_밝힌다() {
        // treemap-basic--dark.svg: #4d1037 → hsl(321.64, 65.59%, 28.24%)
        let peer = lighten(Color::rgb(0x4d, 0x10, 0x37), 10.0);
        assert_eq!(
            peer,
            hsl(321.639_344_262_3, 65.591_397_849_5, 28.235_294_117_6)
        );
    }

    #[test]
    fn 순서_척도는_처음_물은_순서로_번호를_준다() {
        let mut scale = OrdinalScale::default();
        assert_eq!(scale.index(""), 0);
        assert_eq!(scale.index("A"), 1);
        assert_eq!(scale.index(""), 0);
    }
}
