//! `Mod+Alt`을 다른 키 없이 눌렀다 뗄 때만 워크스페이스 목록으로 포커스를 옮기는 판정.
//! 다른 키와 함께 쓰인 `Mod+Alt`은 터미널 탭 단축키(`Mod+Alt+{n}`) 같은 조합의 일부라 목록 이동이 아니다.

use super::defaults::WORKSPACE_TASK_LIST_FOCUS;
use super::key_event::{KeyPhase, KeyTransition, matches_shortcut_modifiers, normalize_event_key};
use super::notation::ShortcutParts;
use crate::platform::ShortcutPlatform;

#[derive(Debug, Clone)]
pub struct WorkspaceListFocusTracker {
    platform: ShortcutPlatform,
    focus_shortcut: ShortcutParts,
    is_armed: bool,
    is_cancelled: bool,
}

impl WorkspaceListFocusTracker {
    pub fn new(platform: ShortcutPlatform) -> Self {
        Self {
            platform,
            focus_shortcut: ShortcutParts::parse(WORKSPACE_TASK_LIST_FOCUS),
            is_armed: false,
            is_cancelled: false,
        }
    }

    /// 창이 포커스를 잃으면 떼는 입력을 받지 못하므로 처음 상태로 돌린다
    pub fn reset(&mut self) {
        self.is_armed = false;
        self.is_cancelled = false;
    }

    fn mod_key_name(&self) -> &'static str {
        match self.platform {
            ShortcutPlatform::Mac => "Meta",
            ShortcutPlatform::Linux => "Ctrl",
        }
    }

    /// 목록으로 포커스를 옮겨야 하는 순간에만 `true`를 돌려준다
    pub fn handle_key_transition(&mut self, transition: &KeyTransition) -> bool {
        if transition.is_composing {
            self.reset();
            return false;
        }
        if transition.is_auto_repeat {
            return false;
        }
        let key = normalize_event_key(&transition.event.key);
        match transition.phase {
            KeyPhase::Down => {
                self.handle_key_down(transition, &key);
                false
            }
            KeyPhase::Up => self.handle_key_up(transition, &key),
        }
    }

    fn is_focus_key(&self, key: &str) -> bool {
        key == self.mod_key_name() || key == "Alt"
    }

    fn handle_key_down(&mut self, transition: &KeyTransition, key: &str) {
        let modifiers = transition.event.modifiers;
        let has_non_mod_platform_key = match self.platform {
            ShortcutPlatform::Mac => modifiers.has_ctrl,
            ShortcutPlatform::Linux => modifiers.has_meta,
        };
        if !self.is_focus_key(key) || modifiers.has_shift || has_non_mod_platform_key {
            self.is_cancelled = true;
            self.is_armed = false;
        } else if !self.is_cancelled
            && matches_shortcut_modifiers(modifiers, &self.focus_shortcut, self.platform)
        {
            self.is_armed = true;
        }
    }

    fn handle_key_up(&mut self, transition: &KeyTransition, key: &str) -> bool {
        let should_focus = self.is_armed && !self.is_cancelled && self.is_focus_key(key);
        if should_focus {
            self.is_armed = false;
        }
        let modifiers = transition.event.modifiers;
        let has_held_modifier = (modifiers.has_meta && key != "Meta")
            || (modifiers.has_ctrl && key != "Ctrl")
            || (modifiers.has_alt && key != "Alt")
            || (modifiers.has_shift && key != "Shift");
        if !has_held_modifier {
            self.reset();
        }
        should_focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::key_event::test_support::{key_down, key_up};

    #[test]
    fn releasing_mod_alt_alone_focuses_once_on_each_platform() {
        for (platform, mod_name, mod_key) in [
            (ShortcutPlatform::Mac, "meta", "Meta"),
            (ShortcutPlatform::Linux, "ctrl", "Control"),
        ] {
            let mut tracker = WorkspaceListFocusTracker::new(platform);

            assert!(!tracker.handle_key_transition(&key_down("Alt", &format!("{mod_name}+alt"))));
            assert!(tracker.handle_key_transition(&key_up("Alt", mod_name)));
            assert!(!tracker.handle_key_transition(&key_up(mod_key, "")));
        }
    }

    #[test]
    fn another_key_or_a_blur_reset_cancels_the_focus() {
        let mut tracker = WorkspaceListFocusTracker::new(ShortcutPlatform::Mac);
        tracker.handle_key_transition(&key_down("Alt", "meta+alt"));
        tracker.handle_key_transition(&key_down("1", "meta+alt"));
        assert!(!tracker.handle_key_transition(&key_up("Alt", "meta")));

        tracker.handle_key_transition(&key_down("Alt", "meta+alt"));
        tracker.reset();
        assert!(!tracker.handle_key_transition(&key_up("Alt", "meta")));
    }

    #[test]
    fn extra_modifiers_and_partial_releases_do_not_focus() {
        let mut tracker = WorkspaceListFocusTracker::new(ShortcutPlatform::Mac);
        tracker.handle_key_transition(&key_down("Alt", "meta+alt+shift"));
        assert!(!tracker.handle_key_transition(&key_up("Shift", "meta+alt")));
        assert!(!tracker.handle_key_transition(&key_up("Alt", "meta")));
        tracker.handle_key_transition(&key_up("Meta", ""));

        tracker.handle_key_transition(&key_down("Meta", "meta+alt"));
        assert!(tracker.handle_key_transition(&key_up("Meta", "alt")));
    }

    #[test]
    fn ime_composition_resets_the_tracker() {
        let mut tracker = WorkspaceListFocusTracker::new(ShortcutPlatform::Linux);
        tracker.handle_key_transition(&key_down("Alt", "ctrl+alt"));
        let composing_release = KeyTransition {
            is_composing: true,
            ..key_up("Alt", "ctrl")
        };

        assert!(!tracker.handle_key_transition(&composing_release));
        assert!(!tracker.handle_key_transition(&key_up("Alt", "ctrl")));
    }
}
