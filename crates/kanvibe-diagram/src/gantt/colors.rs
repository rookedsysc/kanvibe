//! gantt `styles.js`가 기본·dark 테마에서 낸 색. 값은 기준 SVG `<style>`에서 읽었다.

use crate::scene::Color;
use crate::theme::{Theme, hsl};

/// `.section{opacity:0.2}`
const SECTION_OPACITY: f32 = 0.2;
/// `.grid .tick{opacity:0.8}`
const TICK_OPACITY: f32 = 0.8;
const LIGHT_GREY: Color = Color::rgb(211, 211, 211);
const GREY: Color = Color::rgb(128, 128, 128);
/// 두 테마 모두 `#003163`
const CLICKABLE_TEXT: Color = Color::rgb(0x00, 0x31, 0x63);

/// 작업 막대 한 종류의 칠과 테두리
#[derive(Clone, Copy, Debug)]
pub struct BarColors {
    pub fill: Color,
    pub stroke: Color,
}

#[derive(Clone, Debug)]
pub struct GanttColors {
    pub title: Color,
    pub section_title: Color,
    pub tick_text: Color,
    /// 축 눈금선(`stroke="currentColor"` = 검정)
    pub tick_line: Color,
    /// `section0`..`section3` 줄 바탕(불투명도 포함)
    pub section_rows: [Color; 4],
    pub exclude: Color,
    pub today: Color,
    pub task: BarColors,
    pub active: BarColors,
    pub done: BarColors,
    pub crit: BarColors,
    pub active_crit: BarColors,
    pub done_crit: BarColors,
    /// 막대 안 글자(`taskText`)
    pub text_inside: Color,
    /// `activeText`·`doneText`·`activeCritText`·`doneCritText`
    pub text_tagged: Color,
    /// 막대 밖 글자(`taskTextOutside`)
    pub text_outside: Color,
    /// 막대 밖에 놓인 끝난 작업 글자
    pub text_done_outside: Color,
    /// `click`이 걸린 작업 글자(`taskTextClickableColor`, 굵게)
    pub text_clickable: Color,
    /// `vert` 기준선 테두리와 글자(`vertLineColor`)
    pub vert_line: Color,
}

fn faded(color: Color, opacity: f32) -> Color {
    Color {
        alpha: color.alpha * opacity,
        ..color
    }
}

const fn bar(fill: Color, stroke: Color) -> BarColors {
    BarColors { fill, stroke }
}

impl GanttColors {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => light(),
            Theme::Dark => dark(),
        }
    }
}

fn light() -> GanttColors {
    let text = Color::rgb(0x33, 0x33, 0x33);
    let white = Color::rgb(255, 255, 255);
    let black = Color::rgb(0, 0, 0);
    let task_stroke = Color::rgb(0x53, 0x4f, 0xbc);
    let crit_stroke = Color::rgb(0xff, 0x88, 0x88);
    let active_fill = Color::rgb(0xbf, 0xc7, 0xff);
    GanttColors {
        title: text,
        section_title: text,
        tick_text: faded(text, TICK_OPACITY),
        tick_line: faded(black, TICK_OPACITY),
        section_rows: [
            faded(Color::rgba(102, 102, 255, 0.49), SECTION_OPACITY),
            faded(white, SECTION_OPACITY),
            faded(Color::rgb(0xff, 0xf4, 0x00), SECTION_OPACITY),
            faded(white, SECTION_OPACITY),
        ],
        exclude: Color::rgb(0xee, 0xee, 0xee),
        today: Color::rgb(255, 0, 0),
        task: BarColors {
            fill: Color::rgb(0x8a, 0x90, 0xdd),
            stroke: task_stroke,
        },
        active: bar(active_fill, task_stroke),
        done: bar(LIGHT_GREY, GREY),
        crit: BarColors {
            fill: Color::rgb(255, 0, 0),
            stroke: crit_stroke,
        },
        active_crit: bar(active_fill, crit_stroke),
        done_crit: bar(LIGHT_GREY, crit_stroke),
        text_inside: white,
        text_tagged: black,
        text_outside: black,
        text_done_outside: black,
        text_clickable: CLICKABLE_TEXT,
        vert_line: Color::rgb(0, 0, 0x80),
    }
}

fn dark() -> GanttColors {
    let title = Color::rgb(0xF9, 0xFF, 0xFE);
    let section_alternate = faded(Color::rgb(0x33, 0x33, 0x33), SECTION_OPACITY);
    let white = Color::rgb(255, 255, 255);
    let crit = Color::rgb(0xE8, 0x37, 0x37);
    let active_fill = Color::rgb(0x81, 0xB1, 0xDB);
    GanttColors {
        title,
        section_title: title,
        tick_text: faded(Color::rgb(0xcc, 0xcc, 0xcc), TICK_OPACITY),
        tick_line: faded(Color::rgb(0, 0, 0), TICK_OPACITY),
        section_rows: [
            faded(
                hsl(52.941_176_470_6, 28.813_559_322, 58.431_372_549),
                SECTION_OPACITY,
            ),
            section_alternate,
            faded(Color::rgb(0xEA, 0xE8, 0xD9), SECTION_OPACITY),
            section_alternate,
        ],
        exclude: hsl(52.941_176_470_6, 28.813_559_322, 48.431_372_549),
        today: Color::rgb(0xDB, 0x57, 0x57),
        task: BarColors {
            fill: hsl(180.0, 1.587_301_587_3, 35.352_941_176_5),
            stroke: white,
        },
        active: bar(active_fill, white),
        done: bar(LIGHT_GREY, GREY),
        crit: bar(crit, crit),
        active_crit: bar(active_fill, crit),
        done_crit: bar(LIGHT_GREY, crit),
        text_inside: hsl(28.571_428_571_4, 17.355_371_900_8, 86.274_509_803_9),
        text_tagged: Color::rgb(0x2c, 0x2c, 0x2c),
        text_outside: LIGHT_GREY,
        text_done_outside: LIGHT_GREY,
        text_clickable: CLICKABLE_TEXT,
        vert_line: Color::rgb(0x00, 0xBF, 0xFF),
    }
}
