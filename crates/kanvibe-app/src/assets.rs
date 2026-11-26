//! 앱이 번들하는 아이콘·브랜드 이미지. GPUI Kit 컴포넌트가 쓰는 기본 아이콘은 그 뒤에서 찾는다.
//! 아이콘 SVG는 TS 컴포넌트의 인라인 SVG 경로를 그대로 옮겼다(파일 이름이 원래 쓰인 자리를 말한다).

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

pub mod icon_paths {
    /// Board 헤더 프로젝트 스캔 버튼
    pub const FOLDER_PLUS: &str = "icons/kanvibe/folder-plus.svg";
    /// Board 헤더 설정 버튼
    pub const SETTINGS: &str = "icons/kanvibe/settings.svg";
    /// `NotificationCenterButton`
    pub const BELL: &str = "icons/kanvibe/bell.svg";
    /// `BoardSortPicker` 정렬 아이콘
    pub const SORT_LINES: &str = "icons/kanvibe/sort-lines.svg";
    /// `DesktopFilter` 모니터 아이콘
    pub const MONITOR: &str = "icons/kanvibe/monitor.svg";
    /// `ProjectSelector`·`DesktopFilter`의 12px 펼침 표시
    pub const CHEVRON_DOWN_SMALL: &str = "icons/kanvibe/chevron-down-small.svg";
}

/// AI provider 색 아이콘(TS `AiProviderIcon`, `@lobehub/icons-static-svg`). 색이 있어 `svg()`가 아니라 `img()`로 그린다
pub mod provider_icon_paths {
    pub const CLAUDE: &str = "icons/providers/claude-color.svg";
    pub const CODEX: &str = "icons/providers/codex-color.svg";
    pub const GEMINI: &str = "icons/providers/gemini-color.svg";
    pub const OPENCODE: &str = "icons/providers/opencode.svg";
}

pub mod image_paths {
    /// 라이트 테마 로고(원본)
    pub const LOGO: &str = "branding/kanvibe-logo.png";
    /// 다크 테마 로고. CSS `filter: invert(1)`과 같게 RGB만 뒤집고 알파는 둔 파일이다
    pub const LOGO_INVERTED: &str = "branding/kanvibe-logo-inverted.png";
}

const BUNDLED_ASSETS: [(&str, &[u8]); 12] = [
    (
        icon_paths::FOLDER_PLUS,
        include_bytes!("../assets/icons/folder-plus.svg"),
    ),
    (
        icon_paths::SETTINGS,
        include_bytes!("../assets/icons/settings.svg"),
    ),
    (icon_paths::BELL, include_bytes!("../assets/icons/bell.svg")),
    (
        icon_paths::SORT_LINES,
        include_bytes!("../assets/icons/sort-lines.svg"),
    ),
    (
        icon_paths::MONITOR,
        include_bytes!("../assets/icons/monitor.svg"),
    ),
    (
        icon_paths::CHEVRON_DOWN_SMALL,
        include_bytes!("../assets/icons/chevron-down-small.svg"),
    ),
    (
        provider_icon_paths::CLAUDE,
        include_bytes!("../assets/icons/providers/claude-color.svg"),
    ),
    (
        provider_icon_paths::CODEX,
        include_bytes!("../assets/icons/providers/codex-color.svg"),
    ),
    (
        provider_icon_paths::GEMINI,
        include_bytes!("../assets/icons/providers/gemini-color.svg"),
    ),
    (
        provider_icon_paths::OPENCODE,
        include_bytes!("../assets/icons/providers/opencode.svg"),
    ),
    (
        image_paths::LOGO,
        include_bytes!("../assets/branding/kanvibe-logo.png"),
    ),
    (
        image_paths::LOGO_INVERTED,
        include_bytes!("../assets/branding/kanvibe-logo-inverted.png"),
    ),
];

/// 앱 번들 자산을 먼저 찾고, 없으면 GPUI Kit 기본 자산으로 넘긴다
pub struct KanvibeAssets {
    kit_assets: gpui_kit::assets::Assets,
}

impl KanvibeAssets {
    pub fn new() -> Self {
        Self {
            kit_assets: gpui_kit::assets::Assets::new(""),
        }
    }
}

impl Default for KanvibeAssets {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetSource for KanvibeAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = BUNDLED_ASSETS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        self.kit_assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names: Vec<SharedString> = BUNDLED_ASSETS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect();
        names.extend(self.kit_assets.list(path)?);
        Ok(names)
    }
}
