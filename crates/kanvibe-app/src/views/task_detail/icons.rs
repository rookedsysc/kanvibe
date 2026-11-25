//! 작업 상세 화면 전용 아이콘. TS가 그린 인라인 SVG와 Hugeicons 경로를 그대로 옮겼다.
//! GPUI `svg()`는 번들 자산 경로만 받으므로, 색을 넣은 SVG 문서를 만들어 이미지로 그린다(색·크기마다 한 번 만든다).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{Image, ImageFormat, Img, Pixels, Rgba, Styled, img};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailIcon {
    /// Hugeicons `InformationCircleIcon`(dock 작업 정보)
    InformationCircle,
    /// TS `AntennaSignalIcon`(dock 상태 패널)
    AntennaSignal,
    /// Hugeicons `Chatting01Icon`(dock AI 대화)
    Chatting,
    /// TS `PullRequestIcon`
    PullRequest,
    /// Hugeicons `Activity03Icon`(dock 실행 중 세션)
    Activity,
    /// Hugeicons `SourceCodeIcon`(dock VS Code)
    SourceCode,
    /// Hugeicons `ChartHistogramIcon`(dock 사용량)
    ChartHistogram,
    /// dock 보드 복귀 `M10 12L6 8L10 4`
    BackChevron,
    /// 타이틀 카드 브랜치 작업 보기 `M6 12L10 8L6 4`
    ForwardChevron,
    /// 정보 카드 프로젝트 바로가기(굵은 선) `M6 4L10 8L6 12`
    ForwardChevronBold,
    /// Hugeicons `CheckmarkCircle02Icon`(hooks 전체 상태: 모두 설치)
    CheckmarkCircle,
    /// Hugeicons `Clock01Icon`(hooks 전체 상태: 일부 설치)
    Clock,
    /// Hugeicons `AlertCircleIcon`(hooks 전체 상태: 하나도 없음)
    AlertCircle,
    /// `TaskDiffStats` FileDiffIcon
    FileDiff,
    /// 같은 프로젝트 작업 모달 닫기 `M12 4L4 12M4 4L12 12`
    Close,
    /// 네이티브 select 화살표(굵은 아래 꺾쇠)
    SelectChevron,
}

impl DetailIcon {
    fn svg_body(self) -> (&'static str, &'static str, &'static str) {
        // (viewBox, stroke-width, 본문)
        match self {
            Self::InformationCircle => (
                "0 0 24 24",
                "1.6",
                concat!(
                    r#"<circle cx="12" cy="12" r="10"/><path d="M12 16V12"/>"#,
                    r#"<path d="M12.125 8.25H12M12.25 8.25C12.25 8.11193 12.1381 8 12 8C11.8619 8 11.75 8.11193 11.75 8.25C11.75 8.38807 11.8619 8.5 12 8.5C12.1381 8.5 12.25 8.38807 12.25 8.25Z"/>"#,
                ),
            ),
            Self::AntennaSignal => (
                "0 0 24 24",
                "1.7",
                concat!(
                    r#"<path d="M12 19v-7"/><path d="m9 22 3-3 3 3"/>"#,
                    r#"<circle cx="12" cy="10" r="1.6" fill="currentColor" stroke="none"/>"#,
                    r#"<path d="M8.7 13.1a5 5 0 0 1 0-6.2"/><path d="M15.3 6.9a5 5 0 0 1 0 6.2"/>"#,
                    r#"<path d="M5.8 15.8a9 9 0 0 1 0-11.6"/><path d="M18.2 4.2a9 9 0 0 1 0 11.6"/>"#,
                ),
            ),
            Self::Chatting => (
                "0 0 24 24",
                "1.6",
                concat!(
                    r#"<path d="M20 9C19.2048 5.01455 15.5128 2 11.0793 2C6.06549 2 2 5.85521 2 10.61C2 12.8946 2.93819 14.9704 4.46855 16.5108C4.80549 16.85 5.03045 17.3134 4.93966 17.7903C4.78982 18.5701 4.45026 19.2975 3.95305 19.9037C5.26123 20.1449 6.62147 19.9277 7.78801 19.3127C8.20039 19.0954 8.40657 18.9867 8.55207 18.9646C8.65392 18.9492 8.78659 18.9636 9 19.0002"/>"#,
                    r#"<path d="M11 16.2617C11 19.1674 13.4628 21.5234 16.5 21.5234C16.8571 21.5238 17.2132 21.4908 17.564 21.425C17.8165 21.3775 17.9428 21.3538 18.0309 21.3673C18.119 21.3807 18.244 21.4472 18.4938 21.58C19.2004 21.9558 20.0244 22.0885 20.8169 21.9411C20.5157 21.5707 20.31 21.1262 20.2192 20.6496C20.1642 20.3582 20.3005 20.075 20.5046 19.8677C21.4317 18.9263 22 17.6578 22 16.2617C22 13.356 19.5372 11 16.5 11C13.4628 11 11 13.356 11 16.2617Z"/>"#,
                ),
            ),
            Self::PullRequest => (
                "0 0 24 24",
                "2",
                concat!(
                    r#"<circle cx="6" cy="18" r="3"/><circle cx="18" cy="6" r="3"/><path d="M6 15V6"/>"#,
                    r#"<path d="M18 9v1.5A5.5 5.5 0 0 1 12.5 16H9"/><path d="m12 13-3 3 3 3"/>"#,
                ),
            ),
            Self::Activity => (
                "0 0 24 24",
                "1.6",
                concat!(
                    r#"<path d="M4.31802 19.682C3 18.364 3 16.2426 3 12C3 7.75736 3 5.63604 4.31802 4.31802C5.63604 3 7.75736 3 12 3C16.2426 3 18.364 3 19.682 4.31802C21 5.63604 21 7.75736 21 12C21 16.2426 21 18.364 19.682 19.682C18.364 21 16.2426 21 12 21C7.75736 21 5.63604 21 4.31802 19.682Z"/>"#,
                    r#"<path d="M6 12H8.5L10.5 8L13.5 16L15.5 12H18"/>"#,
                ),
            ),
            Self::SourceCode => (
                "0 0 24 24",
                "1.6",
                concat!(
                    r#"<path d="M17 8L18.8398 9.85008C19.6133 10.6279 20 11.0168 20 11.5C20 11.9832 19.6133 12.3721 18.8398 13.1499L17 15"/>"#,
                    r#"<path d="M7 8L5.16019 9.85008C4.38673 10.6279 4 11.0168 4 11.5C4 11.9832 4.38673 12.3721 5.16019 13.1499L7 15"/>"#,
                    r#"<path d="M14.5 4L9.5 20"/>"#,
                ),
            ),
            Self::ChartHistogram => (
                "0 0 24 24",
                "1.6",
                concat!(
                    r#"<path d="M21 21H10C6.70017 21 5.05025 21 4.02513 19.9749C3 18.9497 3 17.2998 3 14V3"/>"#,
                    r#"<path d="M13 10L13 21"/><path d="M18 13L18 21"/><path d="M8 13L8 20"/>"#,
                    r#"<path d="M21 7.98693C19.16 7.98693 17.1922 8.24252 15.8771 6.49346C14.3798 4.50218 11.6202 4.50218 10.1229 6.49346C8.80782 8.24252 6.84003 7.98693 5 7.98693H3"/>"#,
                ),
            ),
            Self::BackChevron => ("0 0 16 16", "1.5", r#"<path d="M10 12L6 8L10 4"/>"#),
            Self::ForwardChevron => ("0 0 16 16", "1.5", r#"<path d="M6 12L10 8L6 4"/>"#),
            Self::ForwardChevronBold => ("0 0 16 16", "2", r#"<path d="M6 4L10 8L6 12"/>"#),
            Self::CheckmarkCircle => (
                "0 0 24 24",
                "2",
                concat!(
                    r#"<path d="M22 12C22 6.47715 17.5228 2 12 2C6.47715 2 2 6.47715 2 12C2 17.5228 6.47715 22 12 22C17.5228 22 22 17.5228 22 12Z"/>"#,
                    r#"<path d="M8 12.5L10.5 15L16 9"/>"#,
                ),
            ),
            Self::Clock => (
                "0 0 24 24",
                "2",
                r#"<circle cx="12" cy="12" r="10"/><path d="M12 8V12L14 14"/>"#,
            ),
            Self::AlertCircle => (
                "0 0 24 24",
                "2",
                concat!(
                    r#"<circle cx="12" cy="12" r="10"/><path d="M12 8V12"/>"#,
                    r#"<path d="M12.125 15.75H12M12.25 15.75C12.25 15.8881 12.1381 16 12 16C11.8619 16 11.75 15.8881 11.75 15.75C11.75 15.6119 11.8619 15.5 12 15.5C12.1381 15.5 12.25 15.6119 12.25 15.75Z"/>"#,
                ),
            ),
            Self::Close => ("0 0 16 16", "1.5", r#"<path d="M12 4L4 12M4 4L12 12"/>"#),
            Self::SelectChevron => ("0 0 16 16", "3.2", r#"<path d="M2.5 5L8 10.5L13.5 5"/>"#),
            Self::FileDiff => (
                "0 0 24 24",
                "2",
                concat!(
                    r#"<circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/>"#,
                    r#"<path d="M6 8.5v4c0 2 1.5 3.5 3.5 3.5H14"/><path d="M15 13l3 3-3 3"/>"#,
                ),
            ),
        }
    }

    fn document(self, color: Rgba, size: f32) -> String {
        let (view_box, stroke_width, body) = self.svg_body();
        let hex = color_hex(color);
        let body = body.replace("currentColor", &hex);
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="{view_box}" fill="none" stroke="{hex}" stroke-width="{stroke_width}" stroke-linecap="round" stroke-linejoin="round" opacity="{opacity}">{body}</svg>"#,
            opacity = color.a,
        )
    }
}

fn color_hex(color: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

type IconKey = (DetailIcon, [u32; 4], u32);

thread_local! {
    static ICON_IMAGES: RefCell<HashMap<IconKey, Arc<Image>>> = RefCell::new(HashMap::new());
}

/// 색과 크기를 넣은 아이콘. 같은 조합은 처음 만든 이미지를 다시 쓴다
pub fn detail_icon(icon: DetailIcon, color: Rgba, size: Pixels) -> Img {
    let size_value = f32::from(size);
    let key = (
        icon,
        [color.r, color.g, color.b, color.a].map(f32::to_bits),
        size_value.to_bits(),
    );
    let image = ICON_IMAGES.with(|images| {
        images
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                Arc::new(Image::from_bytes(
                    ImageFormat::Svg,
                    icon.document(color, size_value).into_bytes(),
                ))
            })
            .clone()
    });
    img(image).size(size).flex_none()
}
