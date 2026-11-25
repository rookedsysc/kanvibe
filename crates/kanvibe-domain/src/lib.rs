//! KanVibe의 UI와 DB에 의존하지 않는 모델과 순수 로직.
//! DB에 저장되거나 모바일로 나가는 값은 TS 앱과 같은 serde 이름을 써서 바이트 단위로 호환된다.

pub mod app_setting;
pub mod background_sync;
pub mod board_sort;
pub mod i18n;
pub mod json_presence;
pub mod locale_order;
pub mod notifications;
pub mod pane_chat;
pub mod pane_layout;
pub mod platform;
pub mod project;
pub mod release_updates;
pub mod shortcuts;
pub mod task;
pub mod task_detail_dock;
pub mod task_detail_route;
pub mod task_diff_stats;
pub mod task_notifications;
pub mod terminal_tabs;
