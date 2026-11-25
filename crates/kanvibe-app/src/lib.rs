//! KanVibe GPUI 데스크톱 앱. Electron 렌더러(`src/desktop/renderer`)와 main 창 관리를 대신한다.

pub mod app_window;
pub mod assets;
pub mod components;
pub mod fonts;
pub mod navigation;
pub mod notifications;
pub mod qa;
pub mod routes;
pub mod services;
pub mod shortcuts;
pub mod theme;
pub mod views;
pub mod window_open;
pub mod windows;

use std::sync::Arc;

use gpui_kit::{App, TextRenderingMode, WindowAppearance};

use crate::qa::faults::FaultRegistry;
use crate::qa::options::LaunchOptions;
use crate::qa::targets::QaTargetRegistry;
use crate::qa::{AppClock, NetworkResponseTable, QaRuntime};
use crate::services::AppServices;
use crate::shortcuts::ShortcutState;
use crate::theme::ThemeTokens;
use crate::windows::WindowRegistry;

/// 창을 열기 전 앱 전역을 준비한다. QA 입력은 모두 `launch`에서 오고 프로세스 환경은 건드리지 않는다.
pub fn init_app(launch: &LaunchOptions, cx: &mut App) -> anyhow::Result<()> {
    gpui_kit::init(cx);
    fonts::register_bundled_fonts(cx)?;
    // 기준 이미지는 그레이스케일 안티에일리어싱(fontconfig rgba none)으로 찍혔다
    cx.set_text_rendering_mode(TextRenderingMode::Grayscale);

    let database = launch
        .data_dir
        .as_deref()
        .map(AppServices::open_database)
        .transpose()?;
    let (faults, clock) = install_qa_runtime(launch, cx)?;
    let services = AppServices::new(database, faults, clock);

    let is_system_light = cx.window_appearance() == WindowAppearance::Light
        || cx.window_appearance() == WindowAppearance::VibrantLight;
    cx.set_global(ThemeTokens::for_mode(
        services.theme_preference().resolve(is_system_light),
    ));
    cx.set_global(ShortcutState {
        bindings: services.shortcut_bindings(),
        platform: services.shortcut_platform,
        capturing_windows: Default::default(),
    });
    cx.set_global(services);
    cx.set_global(WindowRegistry::default());
    cx.set_global(notifications::NotificationCenter::default());
    shortcuts::install_shortcut_dispatch(cx).detach();
    notifications::install_notification_click_routing(cx);
    Ok(())
}

fn install_qa_runtime(
    launch: &LaunchOptions,
    cx: &mut App,
) -> anyhow::Result<(Option<Arc<FaultRegistry>>, AppClock)> {
    let Some(qa) = launch.qa.clone() else {
        return Ok((None, AppClock::system()));
    };
    let faults = match qa.faults_path.as_deref() {
        Some(path) => FaultRegistry::load_file(path)?,
        None => FaultRegistry::default(),
    };
    let network_responses = match qa.network_responses_path.as_deref() {
        Some(path) => NetworkResponseTable::load_file(path)?,
        None => NetworkResponseTable::default(),
    };
    let clock = AppClock::fixed(qa.fixed_now_ms);
    cx.set_global(QaTargetRegistry::default());
    cx.set_global(QaRuntime {
        options: qa,
        network_responses,
    });
    Ok((Some(Arc::new(faults)), clock))
}
