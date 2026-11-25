//! 작업 상세 터미널 위 pane 채팅(TS `TerminalPaneChatLayer`·`PaneChatView`·`PaneChatComposer`·`PaneChatPromptCard`·
//! `PaneChatModelPicker`·`usePaneChat`). pane에서 도는 에이전트 CLI의 대화를 채팅 화면으로 보이고, 입력은 서비스가 확인한
//! 같은 pane·같은 CLI 프로세스에만 넣는다. 모든 서비스 호출은 TS IPC 이름 그대로 [`backend`] 한 곳을 지난다.

mod actions;
mod attachments;
pub mod backend;
mod body;
pub mod client;
mod composer;
mod composer_render;
mod composer_suggestions;
pub mod drafts;
mod host;
mod icons;
mod inspect;
mod layer;
mod message;
mod model_picker;
pub mod preferences;
mod prompt_card;
mod render;
pub mod session;
pub mod suggestions;
mod text_menu;
mod toggle;
mod transcript;
mod view;
mod widgets;

pub use attachments::{PickedFile, data_url, mime_type_for};
pub use body::{
    BodyBlock, RevealControl, UnavailableCard, reveal_message_key, unavailable_message_key,
};
pub use composer::{COMPOSER_MIN_HEIGHT, CompletionKey};
pub use composer_render::MODEL_TRIGGER_SELECTOR;
pub use host::terminal_screen_target;
pub use layer::{
    PaneArea, PaneChatLayer, PaneChatLayerEvent, PaneChatLayerProps, TerminalChatGrid, pane_area,
};
pub use model_picker::ModelPickerState;
pub use prompt_card::{PromptCardModel, PromptOptionModel};
pub use view::{PaneChatView, PaneChatViewEvent, PaneChatViewProps};
