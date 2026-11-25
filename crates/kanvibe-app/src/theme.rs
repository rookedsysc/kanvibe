//! `src/styles/globals.css`의 CSS 사용자 속성을 옮긴 의미 기반 테마 토큰.
//!
//! CSS 캐스케이드를 그대로 재현한다. `:root, :root[data-theme="dark"]` 블록이 기본(다크)이고,
//! `:root[data-theme="light"]` 블록은 일부 토큰만 덮는다. 그래서 라이트 테마에서도 status·agent tag·priority(medium/high)·
//! project group·terminal 색은 다크 값이 남는다(계획 AD-12). 다른 토큰을 `var()`로 가리키는 토큰(예: `priority-low-bg`,
//! `syntax-*`)은 요소에서 계산되므로 테마가 바뀐 값을 따라간다.

use gpui_kit::{Global, Pixels, Rgba, px};

/// 앱에 실제로 적용되는 테마. HTML의 `data-theme` 속성 값에 해당한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// AppSettings `theme_preference`에 저장되는 사용자 선택
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemePreference {
    System,
    Dark,
    Light,
}

impl ThemePreference {
    pub const APP_SETTING_KEY: &'static str = "theme_preference";

    /// 알 수 없는 값은 TS `getThemePreference`처럼 `system`으로 본다
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("dark") => Self::Dark,
            Some("light") => Self::Light,
            _ => Self::System,
        }
    }

    /// `system`이면 OS가 라이트일 때만 라이트를 쓴다(`prefers-color-scheme: light` 미디어 쿼리와 같은 판정)
    pub fn resolve(self, is_system_light: bool) -> ThemeMode {
        match self {
            Self::Dark => ThemeMode::Dark,
            Self::Light => ThemeMode::Light,
            Self::System if is_system_light => ThemeMode::Light,
            Self::System => ThemeMode::Dark,
        }
    }
}

/// `rgba(r, g, b, a)` CSS 값. 알파는 CSS 그대로 소수로 둔다.
const fn rgba_css(red: u8, green: u8, blue: u8, alpha: f32) -> Rgba {
    Rgba {
        r: red as f32 / 255.0,
        g: green as f32 / 255.0,
        b: blue as f32 / 255.0,
        a: alpha,
    }
}

/// `#RRGGBB` CSS 값
const fn hex(value: u32) -> Rgba {
    rgba_css(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
        1.0,
    )
}

/// 배경과 글자 한 쌍(태그·우선순위 배지)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorPair {
    pub background: Rgba,
    pub text: Rgba,
}

const fn pair(background: Rgba, text: Rgba) -> ColorPair {
    ColorPair { background, text }
}

/// CSS `box-shadow` 한 겹. `spread`가 음수이면 그림자가 안쪽으로 줄어든다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowLayer {
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
}

const fn shadow(offset_y: f32, blur: f32, spread: f32, color: Rgba) -> ShadowLayer {
    ShadowLayer {
        offset_y,
        blur,
        spread,
        color,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrandColors {
    pub primary: Rgba,
    pub hover: Rgba,
    pub active: Rgba,
    pub subtle: Rgba,
    pub muted: Rgba,
}

/// 경고가 아닌 버튼 표면(`#202632` 계열)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeutralButtonColors {
    pub base: Rgba,
    pub hover: Rgba,
    pub active: Rgba,
    pub subtle: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackgroundColors {
    pub page: Rgba,
    pub surface: Rgba,
    pub overlay: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextColors {
    pub primary: Rgba,
    pub secondary: Rgba,
    pub muted: Rgba,
    pub inverse: Rgba,
    pub brand: Rgba,
    pub link: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderColors {
    pub default: Rgba,
    pub subtle: Rgba,
    pub strong: Rgba,
    pub brand: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusColors {
    pub todo: Rgba,
    pub progress: Rgba,
    pub pending: Rgba,
    pub review: Rgba,
    pub done: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
    pub error: Rgba,
    pub info: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntaxColors {
    pub comment: Rgba,
    pub keyword: Rgba,
    pub string: Rgba,
    pub number: Rgba,
    pub title: Rgba,
    pub deletion: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TagColors {
    pub claude: ColorPair,
    pub gemini: ColorPair,
    pub codex: ColorPair,
    pub session: ColorPair,
    pub ssh: ColorPair,
    pub project: ColorPair,
    pub branch: ColorPair,
    pub neutral: ColorPair,
    pub pull_request: ColorPair,
    pub base_branch: ColorPair,
}

/// 에이전트 범주색. 각 CLI 브랜드색이라 테마와 상관없이 같다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgentColors {
    pub claude: Rgba,
    pub codex: Rgba,
    pub opencode: Rgba,
    pub gemini: Rgba,
}

/// 프로젝트 그룹 파스텔(`group-0..7`)의 테두리와 배경
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectGroupColors {
    pub border: Rgba,
    pub background: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriorityColors {
    pub low: ColorPair,
    pub medium: ColorPair,
    pub high: ColorPair,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalChromeColors {
    pub chrome: Rgba,
    pub background: Rgba,
    pub text: Rgba,
    pub traffic_close: Rgba,
    pub traffic_minimize: Rgba,
    pub traffic_maximize: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowTokens {
    pub xs: [ShadowLayer; 1],
    pub sm: [ShadowLayer; 1],
    pub md: [ShadowLayer; 1],
    pub lg: [ShadowLayer; 1],
    /// 키보드로 포커스한 칸반 카드의 링과 번짐(`--shadow-kanban-task-focus`)
    pub kanban_task_focus: [ShadowLayer; 3],
}

/// 모든 의미 기반 색 토큰. 화면 코드는 hex 값 대신 이 구조체의 이름을 쓴다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTokens {
    pub mode: ThemeMode,
    pub brand: BrandColors,
    pub button_neutral: NeutralButtonColors,
    pub background: BackgroundColors,
    pub text: TextColors,
    pub border: BorderColors,
    pub status: StatusColors,
    pub syntax: SyntaxColors,
    pub tag: TagColors,
    pub agent: AgentColors,
    pub project_groups: [ProjectGroupColors; 8],
    pub group_line: Rgba,
    pub priority: PriorityColors,
    pub terminal: TerminalChromeColors,
    pub shadow: ShadowTokens,
}

impl Global for ThemeTokens {}

const POINT_BLUE: Rgba = hex(0x0064FF);
const BUTTON_GRAY: Rgba = hex(0x202632);
const GREEN_50: Rgba = hex(0xE6F4EA);
const GREEN_700: Rgba = hex(0x188038);

const DARK_STATUS: StatusColors = StatusColors {
    todo: hex(0x747985),
    progress: hex(0xf6c35f),
    pending: hex(0x9b8cff),
    review: POINT_BLUE,
    done: hex(0x65d08a),
    success: hex(0x65d08a),
    warning: hex(0xf6c35f),
    error: hex(0xff7d73),
    info: POINT_BLUE,
};

const AGENT: AgentColors = AgentColors {
    claude: hex(0xD97757),
    codex: hex(0x5EC8D8),
    opencode: hex(0xA78BFA),
    gemini: hex(0x4285F4),
};

const DARK_PROJECT_GROUPS: [ProjectGroupColors; 8] = [
    group(rgba_css(255, 154, 204, 0.4), rgba_css(255, 154, 204, 0.08)),
    group(rgba_css(147, 197, 253, 0.38), rgba_css(147, 197, 253, 0.08)),
    group(rgba_css(134, 239, 172, 0.36), rgba_css(134, 239, 172, 0.08)),
    group(rgba_css(196, 181, 253, 0.38), rgba_css(196, 181, 253, 0.08)),
    group(rgba_css(253, 186, 116, 0.34), rgba_css(253, 186, 116, 0.08)),
    group(rgba_css(253, 224, 71, 0.28), rgba_css(253, 224, 71, 0.07)),
    group(rgba_css(94, 234, 212, 0.34), rgba_css(94, 234, 212, 0.08)),
    group(rgba_css(165, 180, 252, 0.36), rgba_css(165, 180, 252, 0.08)),
];

const fn group(border: Rgba, background: Rgba) -> ProjectGroupColors {
    ProjectGroupColors { border, background }
}

/// 라이트 블록이 덮지 않아 두 테마가 공유하는 다크 터미널 크롬
const TERMINAL: TerminalChromeColors = TerminalChromeColors {
    chrome: hex(0x0f1014),
    background: hex(0x050608),
    text: hex(0xa0a7b4),
    traffic_close: hex(0xff5f56),
    traffic_minimize: hex(0xffbd2e),
    traffic_maximize: hex(0x27c93f),
};

impl ThemeTokens {
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        }
    }

    fn dark() -> Self {
        let brand = BrandColors {
            primary: POINT_BLUE,
            hover: hex(0x2B7DFF),
            active: hex(0x0052D9),
            subtle: rgba_css(0, 100, 255, 0.16),
            muted: rgba_css(0, 100, 255, 0.24),
        };
        let text = TextColors {
            primary: hex(0xf3f4f7),
            secondary: hex(0xb0b4bd),
            muted: hex(0x717680),
            inverse: hex(0xffffff),
            brand: POINT_BLUE,
            link: POINT_BLUE,
        };
        let button_neutral = NeutralButtonColors {
            base: BUTTON_GRAY,
            hover: hex(0x2A3242),
            active: hex(0x161B24),
            subtle: rgba_css(32, 38, 50, 0.3),
        };
        Self {
            mode: ThemeMode::Dark,
            brand,
            button_neutral,
            background: BackgroundColors {
                page: hex(0x090a0d),
                surface: hex(0x111216),
                overlay: rgba_css(0, 0, 0, 0.62),
            },
            text,
            border: BorderColors {
                default: hex(0x24262d),
                subtle: hex(0x1a1c22),
                strong: hex(0x353842),
                brand: rgba_css(0, 100, 255, 0.62),
            },
            status: DARK_STATUS,
            syntax: syntax_colors(&text, &DARK_STATUS),
            tag: dark_tags(&brand, &button_neutral),
            agent: AGENT,
            project_groups: DARK_PROJECT_GROUPS,
            group_line: hex(0x3b3f49),
            priority: priority_colors(&brand),
            terminal: TERMINAL,
            shadow: dark_shadows(),
        }
    }

    /// 다크 값 위에 `:root[data-theme="light"]` 블록이 덮는 토큰만 바꾼다
    fn light() -> Self {
        let dark = Self::dark();
        let brand = BrandColors {
            primary: POINT_BLUE,
            hover: hex(0x0052D9),
            active: hex(0x0044B8),
            subtle: hex(0xEAF2FF),
            muted: hex(0xD6E6FF),
        };
        let text = TextColors {
            primary: hex(0x17181c),
            secondary: hex(0x5f636d),
            muted: hex(0x8a909b),
            inverse: hex(0xffffff),
            brand: POINT_BLUE,
            link: POINT_BLUE,
        };
        let button_neutral = NeutralButtonColors {
            subtle: rgba_css(32, 38, 50, 0.1),
            ..dark.button_neutral
        };
        Self {
            mode: ThemeMode::Light,
            brand,
            button_neutral,
            background: BackgroundColors {
                page: hex(0xf7f8fa),
                surface: hex(0xffffff),
                overlay: rgba_css(15, 17, 22, 0.42),
            },
            text,
            border: BorderColors {
                default: hex(0xe2e4e8),
                subtle: hex(0xeef0f3),
                strong: hex(0xc8ccd3),
                brand: POINT_BLUE,
            },
            syntax: syntax_colors(&text, &dark.status),
            tag: light_tags(&dark.tag, &brand, &button_neutral),
            group_line: hex(0xcdd1d8),
            priority: priority_colors(&brand),
            shadow: light_shadows(),
            ..dark
        }
    }
}

/// 다크 블록의 `--shadow-*`
fn dark_shadows() -> ShadowTokens {
    ShadowTokens {
        xs: [shadow(1.0, 1.0, 0.0, rgba_css(0, 0, 0, 0.24))],
        sm: [shadow(1.0, 2.0, 0.0, rgba_css(0, 0, 0, 0.24))],
        md: [shadow(12.0, 30.0, 0.0, rgba_css(0, 0, 0, 0.26))],
        lg: [shadow(22.0, 60.0, 0.0, rgba_css(0, 0, 0, 0.34))],
        kanban_task_focus: [
            shadow(0.0, 0.0, 1.0, rgba_css(0, 100, 255, 0.95)),
            shadow(0.0, 0.0, 5.0, rgba_css(0, 100, 255, 0.38)),
            shadow(16.0, 36.0, 0.0, rgba_css(0, 100, 255, 0.38)),
        ],
    }
}

/// 라이트 블록의 `--shadow-*`
fn light_shadows() -> ShadowTokens {
    ShadowTokens {
        xs: [shadow(1.0, 1.0, 0.0, rgba_css(17, 24, 39, 0.04))],
        sm: [shadow(1.0, 2.0, 0.0, rgba_css(17, 24, 39, 0.06))],
        md: [shadow(12.0, 28.0, 0.0, rgba_css(17, 24, 39, 0.08))],
        lg: [shadow(22.0, 60.0, 0.0, rgba_css(17, 24, 39, 0.14))],
        kanban_task_focus: [
            shadow(0.0, 0.0, 1.0, POINT_BLUE),
            shadow(0.0, 0.0, 5.0, rgba_css(0, 100, 255, 0.18)),
            shadow(12.0, 28.0, 0.0, rgba_css(0, 100, 255, 0.22)),
        ],
    }
}

/// `--color-syntax-*`는 다른 토큰을 `var()`로 가리켜 테마의 글자색을 따라간다
fn syntax_colors(text: &TextColors, status: &StatusColors) -> SyntaxColors {
    SyntaxColors {
        comment: text.muted,
        keyword: status.pending,
        string: status.success,
        number: status.warning,
        title: text.link,
        deletion: status.error,
    }
}

fn dark_tags(brand: &BrandColors, button_neutral: &NeutralButtonColors) -> TagColors {
    TagColors {
        claude: pair(rgba_css(255, 125, 115, 0.12), hex(0xffaaa2)),
        gemini: pair(rgba_css(110, 168, 255, 0.12), hex(0x9fc8ff)),
        codex: pair(rgba_css(101, 208, 138, 0.12), hex(0x9be6b4)),
        session: pair(hex(0xffffff), hex(0x202124)),
        ssh: pair(rgba_css(101, 208, 138, 0.16), hex(0x9be6b4)),
        project: pair(button_neutral.base, hex(0xffffff)),
        branch: pair(rgba_css(255, 255, 255, 0.06), hex(0xaeb3bd)),
        neutral: pair(rgba_css(255, 255, 255, 0.07), hex(0xaeb3bd)),
        pull_request: pair(brand.subtle, brand.primary),
        base_branch: pair(button_neutral.base, hex(0xffffff)),
    }
}

/// 라이트 블록은 session·ssh·project·pr·base 태그만 덮는다. claude·gemini·codex·branch·neutral은 다크 값이 남는다.
fn light_tags(
    dark: &TagColors,
    brand: &BrandColors,
    button_neutral: &NeutralButtonColors,
) -> TagColors {
    TagColors {
        session: pair(button_neutral.base, hex(0xffffff)),
        ssh: pair(GREEN_50, GREEN_700),
        project: pair(button_neutral.base, hex(0xffffff)),
        pull_request: pair(brand.subtle, brand.primary),
        base_branch: pair(button_neutral.base, hex(0xffffff)),
        ..*dark
    }
}

/// low는 `brand-subtle`·`brand-primary`를 가리켜 테마를 따라가고, medium·high는 다크 블록의 고정값이 남는다
fn priority_colors(brand: &BrandColors) -> PriorityColors {
    PriorityColors {
        low: pair(brand.subtle, brand.primary),
        medium: pair(rgba_css(246, 195, 95, 0.13), hex(0xffd58a)),
        high: pair(rgba_css(255, 125, 115, 0.13), hex(0xffaaa2)),
    }
}

/// `index.html`이 `html`에 고정한 배경. body가 덮지 않는 자리(페이지 스크롤바 홈)에 테마와 상관없이 보인다
pub const CANVAS_BACKGROUND: Rgba = hex(0x090a0d);

/// Tailwind 4px 간격 단위(`--spacing: .25rem`)
pub fn spacing(steps: f32) -> Pixels {
    px(steps * 4.0)
}

/// `rounded-*` 반경. `rounded`(접미사 없음)는 Tailwind가 고정 0.25rem을 쓰고, 나머지는 다크 블록이 덮은 `--radius-*`다.
pub mod radius {
    use gpui_kit::{Pixels, px};

    pub const SM: Pixels = px(3.0);
    pub const BASE: Pixels = px(4.0);
    pub const MD: Pixels = px(6.0);
    pub const LG: Pixels = px(8.0);
    pub const XL: Pixels = px(10.0);
    /// `rounded-full`. 요소 높이의 절반보다 크면 GPUI가 알약 모양으로 자른다
    pub const FULL: Pixels = px(9999.0);
}

/// Tailwind 글자 크기와 줄 높이(`text-*`), body 기본값
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextScale {
    pub font_size: Pixels,
    pub line_height: Pixels,
}

pub mod text_scale {
    use super::TextScale;
    use gpui_kit::px;

    const fn scale(font_size: f32, line_height: f32) -> TextScale {
        TextScale {
            font_size: px(font_size),
            line_height: px(line_height),
        }
    }

    /// body 14px, preflight `line-height: 1.5`
    pub const BODY: TextScale = scale(14.0, 21.0);
    pub const XS: TextScale = scale(12.0, 16.0);
    pub const SM: TextScale = scale(14.0, 20.0);
    pub const BASE: TextScale = scale(16.0, 24.0);
    pub const LG: TextScale = scale(18.0, 28.0);
    pub const XL: TextScale = scale(20.0, 28.0);
    pub const XXL: TextScale = scale(24.0, 32.0);
    /// `text-[11px]` 등 임의 크기는 줄 높이를 따로 주지 않으면 body의 1.5배를 물려받는다
    pub const PX_11: TextScale = scale(11.0, 16.5);
    pub const PX_10: TextScale = scale(10.0, 15.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    // parity: TS-styles-colorTokens-437637
    #[test]
    fn brand_and_neutral_button_use_the_point_colors_in_both_themes() {
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let tokens = ThemeTokens::for_mode(mode);
            assert_eq!(tokens.brand.primary, hex(0x0064FF));
            assert_eq!(tokens.button_neutral.base, hex(0x202632));
            assert_eq!(tokens.tag.project.background, tokens.button_neutral.base);
            assert_eq!(tokens.tag.pull_request.text, tokens.brand.primary);
        }
    }

    #[test]
    fn light_theme_keeps_dark_status_agent_tag_and_priority_values() {
        let dark = ThemeTokens::for_mode(ThemeMode::Dark);
        let light = ThemeTokens::for_mode(ThemeMode::Light);

        assert_eq!(light.status, dark.status);
        assert_eq!(light.tag.claude, dark.tag.claude);
        assert_eq!(light.tag.branch, dark.tag.branch);
        assert_eq!(light.priority.medium, dark.priority.medium);
        assert_eq!(light.priority.high, dark.priority.high);
        assert_eq!(light.project_groups, dark.project_groups);
        assert_eq!(light.terminal, dark.terminal);
        assert_ne!(light.background.page, dark.background.page);
        assert_ne!(light.group_line, dark.group_line);
    }

    // parity: TS-styles-globals-f98c48
    #[test]
    fn light_theme_keeps_project_and_base_tags_on_the_neutral_button() {
        let light = ThemeTokens::for_mode(ThemeMode::Light);

        assert_eq!(light.tag.project, pair(hex(0x202632), hex(0xffffff)));
        assert_eq!(light.tag.base_branch, pair(hex(0x202632), hex(0xffffff)));
    }

    // parity: TS-styles-globals-32f486
    #[test]
    fn session_and_remote_tags_change_with_the_theme() {
        let dark = ThemeTokens::for_mode(ThemeMode::Dark);
        let light = ThemeTokens::for_mode(ThemeMode::Light);

        assert_eq!(dark.tag.session, pair(hex(0xffffff), hex(0x202124)));
        assert_eq!(
            dark.tag.ssh,
            pair(rgba_css(101, 208, 138, 0.16), hex(0x9be6b4))
        );
        assert_eq!(
            light.tag.session,
            pair(light.button_neutral.base, hex(0xffffff))
        );
        assert_eq!(light.tag.ssh, pair(GREEN_50, GREEN_700));
        assert_eq!(dark.tag.pull_request.text, dark.brand.primary);
    }

    // parity: TS-styles-globals-38bfcd
    #[test]
    fn keyboard_focused_kanban_cards_get_a_high_contrast_brand_glow() {
        let dark = ThemeTokens::for_mode(ThemeMode::Dark);
        let light = ThemeTokens::for_mode(ThemeMode::Light);

        assert_eq!(
            dark.shadow.kanban_task_focus[1].color,
            rgba_css(0, 100, 255, 0.38)
        );
        assert_eq!(light.shadow.kanban_task_focus[0].color, light.border.brand);
    }

    // parity: TS-styles-colorTokens-079f05
    #[test]
    fn agent_conventions_document_the_color_tokens() {
        let conventions = include_str!("../../../CLAUDE.md");

        for expected in [
            "#0064FF",
            "#202632",
            "--color-brand-primary",
            "--color-button-neutral-*",
        ] {
            assert!(conventions.contains(expected), "{expected}");
        }
    }

    #[test]
    fn tokens_that_reference_other_tokens_follow_the_active_theme() {
        let light = ThemeTokens::for_mode(ThemeMode::Light);

        assert_eq!(light.priority.low.background, hex(0xEAF2FF));
        assert_eq!(light.syntax.comment, light.text.muted);
        assert_eq!(light.tag.ssh, pair(GREEN_50, GREEN_700));
    }

    #[test]
    fn system_preference_follows_the_os_light_flag() {
        assert_eq!(
            ThemePreference::parse(Some("weird")),
            ThemePreference::System
        );
        assert_eq!(ThemePreference::System.resolve(true), ThemeMode::Light);
        assert_eq!(ThemePreference::System.resolve(false), ThemeMode::Dark);
        assert_eq!(ThemePreference::Dark.resolve(true), ThemeMode::Dark);
    }
}
