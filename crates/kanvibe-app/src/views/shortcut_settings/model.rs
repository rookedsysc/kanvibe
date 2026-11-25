//! 단축키 설정 화면 상태(TS `ShortcutSettingsRoute`의 상태 부분). 녹화든 개별 되돌리기든 같은 관문(충돌 검사)을 지나야
//! 중복 배정이 조용히 생기지 않는다. 저장은 표를 통째로 치환하므로, 저장된 표를 읽지 못한 채로는 저장하지 않는다.

use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::{
    ShortcutBindings, ShortcutCommandGroup, ShortcutCommandId, shortcut_command_definitions,
};
use kanvibe_domain::shortcuts::key_event::{
    ShortcutKeyEvent, capture_shortcut_from_event, is_shortcut_modifier_key,
};

/// 화면 그룹 순서
pub const SHORTCUT_GROUP_ORDER: [ShortcutCommandGroup; 4] = [
    ShortcutCommandGroup::TaskDetailDock,
    ShortcutCommandGroup::TaskDetail,
    ShortcutCommandGroup::Board,
    ShortcutCommandGroup::Terminal,
];

/// 화면 위쪽 경고 띠에 보일 안내
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutNotice {
    /// 이 명령이 이미 그 조합을 쓴다
    Conflict(ShortcutCommandId),
    /// 수정 키가 없거나 담을 수 없는 조합
    Unsupported,
    SaveFailed,
    LoadFailed,
}

/// 녹화 중 누른 키를 어떻게 처리했는지
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingKeyOutcome {
    /// 녹화를 끝내고 이 표를 저장한다
    Save(ShortcutBindings),
    /// 녹화를 취소했다(Esc)
    Cancelled,
    /// 녹화를 이어 간다(수정 키만 눌렀거나 안내를 띄웠다)
    Continue,
}

pub struct ShortcutSettingsModel {
    pub bindings: ShortcutBindings,
    pub platform: ShortcutPlatform,
    pub recording: Option<ShortcutCommandId>,
    pub notice: Option<ShortcutNotice>,
    /// 저장된 표를 읽었는지. 못 읽었으면 녹화·되돌리기·저장을 막는다
    pub is_loaded: bool,
}

impl ShortcutSettingsModel {
    pub fn new(platform: ShortcutPlatform) -> Self {
        Self {
            bindings: ShortcutBindings::defaults(),
            platform,
            recording: None,
            notice: None,
            is_loaded: false,
        }
    }

    pub fn finish_load(&mut self, result: Result<ShortcutBindings, String>) {
        match result {
            Ok(bindings) => {
                self.bindings = bindings;
                self.is_loaded = true;
            }
            Err(_) => {
                self.is_loaded = false;
                self.notice = Some(ShortcutNotice::LoadFailed);
            }
        }
    }

    pub fn is_customized(&self, command_id: ShortcutCommandId) -> bool {
        default_shortcut(command_id) != self.bindings.shortcut(command_id)
    }

    /// "녹화" 버튼. 같은 명령을 다시 누르면 녹화를 멈춘다
    pub fn toggle_recording(&mut self, command_id: ShortcutCommandId) {
        self.notice = None;
        self.recording = if self.recording == Some(command_id) {
            None
        } else {
            Some(command_id)
        };
    }

    /// 녹화 중 키 입력
    pub fn handle_recording_key(&mut self, event: &ShortcutKeyEvent) -> RecordingKeyOutcome {
        let Some(command_id) = self.recording else {
            return RecordingKeyOutcome::Continue;
        };
        if event.key == "Escape" {
            self.notice = None;
            self.recording = None;
            return RecordingKeyOutcome::Cancelled;
        }
        // 조합을 누르는 모든 사용자가 수정 키를 먼저 누른다. 이걸 오류로 보면 녹화마다 거짓 경고가 뜬다
        if is_shortcut_modifier_key(&event.key) {
            return RecordingKeyOutcome::Continue;
        }
        let Some(shortcut) = capture_shortcut_from_event(event, self.platform) else {
            self.notice = Some(ShortcutNotice::Unsupported);
            return RecordingKeyOutcome::Continue;
        };
        if let Some(conflict) = self
            .bindings
            .find_conflict(command_id, &shortcut, self.platform)
        {
            self.notice = Some(ShortcutNotice::Conflict(conflict));
            return RecordingKeyOutcome::Continue;
        }
        self.notice = None;
        self.recording = None;
        match self.prepare_save(command_id, &shortcut) {
            Some(bindings) => RecordingKeyOutcome::Save(bindings),
            None => RecordingKeyOutcome::Cancelled,
        }
    }

    /// 한 명령을 기본값으로 되돌린다. 기본값이 다른 명령과 겹치면 저장하지 않고 알린다
    pub fn reset_command(&mut self, command_id: ShortcutCommandId) -> Option<ShortcutBindings> {
        self.notice = None;
        let shortcut = default_shortcut(command_id);
        if let Some(conflict) = self
            .bindings
            .find_conflict(command_id, shortcut, self.platform)
        {
            self.notice = Some(ShortcutNotice::Conflict(conflict));
            return None;
        }
        self.prepare_save(command_id, shortcut)
    }

    /// 모든 재배정을 지운다
    pub fn reset_all(&mut self) -> Option<ShortcutBindings> {
        self.notice = None;
        self.recording = None;
        self.guard_loaded()?;
        Some(ShortcutBindings::defaults())
    }

    pub fn finish_save(&mut self, saved: ShortcutBindings, result: Result<(), String>) {
        match result {
            Ok(()) => self.bindings = saved,
            Err(_) => self.notice = Some(ShortcutNotice::SaveFailed),
        }
    }

    fn prepare_save(
        &mut self,
        command_id: ShortcutCommandId,
        shortcut: &str,
    ) -> Option<ShortcutBindings> {
        self.guard_loaded()?;
        let mut next = self.bindings.clone();
        next.set_shortcut(command_id, shortcut);
        Some(next)
    }

    /// 읽지 못한 기본값 표를 저장하면 저장돼 있던 재배정이 전부 지워진다
    fn guard_loaded(&mut self) -> Option<()> {
        if self.is_loaded {
            return Some(());
        }
        self.notice = Some(ShortcutNotice::LoadFailed);
        None
    }
}

pub fn default_shortcut(command_id: ShortcutCommandId) -> &'static str {
    shortcut_command_definitions()
        .iter()
        .find(|definition| definition.id == command_id)
        .map(|definition| definition.default_shortcut.as_str())
        .unwrap_or_default()
}
