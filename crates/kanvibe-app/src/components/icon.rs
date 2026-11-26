//! 앱이 번들한 아이콘 SVG(`assets.rs`의 `icon_paths`). TS 인라인 SVG 경로를 그대로 옮긴 파일이다.
//! 새 아이콘은 `assets/icons/`에 SVG를 넣고 `icon_paths`와 여기 `IconName`에 함께 더한다.

use gpui_kit::{Pixels, Styled, Svg, svg};

use crate::assets::icon_paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    FolderPlus,
    Settings,
    Bell,
    SortLines,
    Monitor,
    ChevronDownSmall,
}

impl IconName {
    pub fn asset_path(self) -> &'static str {
        match self {
            Self::FolderPlus => icon_paths::FOLDER_PLUS,
            Self::Settings => icon_paths::SETTINGS,
            Self::Bell => icon_paths::BELL,
            Self::SortLines => icon_paths::SORT_LINES,
            Self::Monitor => icon_paths::MONITOR,
            Self::ChevronDownSmall => icon_paths::CHEVRON_DOWN_SMALL,
        }
    }
}

/// 정사각형 아이콘. GPUI svg는 부모 글자색을 물려받지 않으므로 호출부가 `text_color`를 건다
pub fn icon(name: IconName, size: Pixels) -> Svg {
    svg().path(name.asset_path()).size(size).flex_none()
}
