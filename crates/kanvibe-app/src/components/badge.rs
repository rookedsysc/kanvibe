//! 글자 배지·태그. 색 쌍은 `--color-tag-*-bg`/`--color-tag-*-text` 토큰이다.
//! TS 예: `text-xs font-mono bg-tag-branch-bg text-tag-branch-text px-2 py-0.5 rounded-md`,
//! `text-xs px-2 py-0.5 rounded-full font-medium text-white bg-tag-project-bg`.

use gpui_kit::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div};

use crate::components::TailwindText;
use crate::theme::{ColorPair, ThemeTokens, radius, spacing, text_scale};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Claude,
    Gemini,
    Codex,
    Session,
    Ssh,
    Project,
    Branch,
    Neutral,
    PullRequest,
    BaseBranch,
}

impl BadgeTone {
    fn colors(self, theme: &ThemeTokens) -> ColorPair {
        let tags = &theme.tag;
        match self {
            Self::Claude => tags.claude,
            Self::Gemini => tags.gemini,
            Self::Codex => tags.codex,
            Self::Session => tags.session,
            Self::Ssh => tags.ssh,
            Self::Project => tags.project,
            Self::Branch => tags.branch,
            Self::Neutral => tags.neutral,
            Self::PullRequest => tags.pull_request,
            Self::BaseBranch => tags.base_branch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeShape {
    /// `rounded`(4px)
    Rounded,
    /// `rounded-md`(6px)
    RoundedMedium,
    /// `rounded-full`
    Pill,
}

#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    tone: BadgeTone,
    shape: BadgeShape,
    is_mono: bool,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>, tone: BadgeTone) -> Self {
        Self {
            label: label.into(),
            tone,
            shape: BadgeShape::Rounded,
            is_mono: false,
        }
    }

    pub fn shape(mut self, shape: BadgeShape) -> Self {
        self.shape = shape;
        self
    }

    /// `font-mono`(브랜치·SSH 호스트)
    pub fn mono(mut self) -> Self {
        self.is_mono = true;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeTokens>();
        let colors = self.tone.colors(theme);
        let corner = match self.shape {
            BadgeShape::Rounded => radius::BASE,
            BadgeShape::RoundedMedium => radius::MD,
            BadgeShape::Pill => radius::FULL,
        };
        let badge = div()
            .flex_none()
            .px(spacing(2.0))
            .py(spacing(0.5))
            .rounded(corner)
            .bg(colors.background)
            .text_color(colors.text)
            .text_scale(text_scale::XS)
            .truncate()
            .child(self.label);
        if self.is_mono { badge.mono(400) } else { badge }
    }
}
