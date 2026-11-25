//! 서비스 오류를 라우트가 가리는 TS 오류 이름([`BridgeError`])으로 옮긴다.

use kanvibe_mobile::error::MobileError;
use kanvibe_pane_mirror::PaneMirrorError;
use kanvibe_terminal::pane_chat::service::PaneChatError;

use crate::bridge::BridgeError;

pub fn from_mobile(error: MobileError) -> BridgeError {
    match error {
        MobileError::TaskInput(message) => BridgeError::TaskInput(message),
        MobileError::Feature { status, message } => BridgeError::Feature { status, message },
        MobileError::SessionDependencyMissing(message) => {
            BridgeError::SessionDependencyMissing(message)
        }
        MobileError::TaskWorkspaceFile(reason) => {
            BridgeError::TaskWorkspaceFile(reason.to_string())
        }
        MobileError::AiRequest { status, message } => BridgeError::Ai {
            status: Some(status),
            message,
        },
        MobileError::DirtyWorktree(message) => BridgeError::DirtyWorktree(message),
        MobileError::InvalidWorktree(message) => BridgeError::InvalidWorktree(message),
        MobileError::Internal(message) => BridgeError::Internal(message),
    }
}

pub fn from_mirror(error: PaneMirrorError) -> BridgeError {
    match error {
        PaneMirrorError::SurfaceUnavailable(reason) => {
            BridgeError::SurfaceUnavailable(reason.as_str().to_string())
        }
        PaneMirrorError::PaneNotFound => {
            BridgeError::PaneNotFound(PaneMirrorError::PaneNotFound.to_string())
        }
        PaneMirrorError::LastPane => BridgeError::LastPane(PaneMirrorError::LastPane.to_string()),
        PaneMirrorError::MobileTaskInput(message) => BridgeError::TaskInput(message),
        PaneMirrorError::Failed(message) => BridgeError::Internal(message),
    }
}

pub fn from_pane_chat(error: PaneChatError) -> BridgeError {
    match error {
        PaneChatError::Input(message) => BridgeError::PaneChatInput(message),
        PaneChatError::TargetChanged => {
            BridgeError::PaneChatTargetChanged(PaneChatError::TargetChanged.to_string())
        }
        error @ PaneChatError::DeliveryUncertain { .. } => {
            BridgeError::PaneChatDeliveryUncertain(error.to_string())
        }
        PaneChatError::Other(message) => BridgeError::Internal(message),
    }
}

pub fn from_serde(error: serde_json::Error) -> BridgeError {
    BridgeError::Internal(error.to_string())
}
