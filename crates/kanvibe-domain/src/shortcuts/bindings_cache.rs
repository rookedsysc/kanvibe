//! 키 입력 처리는 동기라 그 순간 저장소를 읽을 수 없다. 그래서 앱이 뜰 때 읽은 단축키 표를 여기에 담아 두고,
//! 설정 화면이 바꿀 때마다 다시 채운다.

use super::bindings::ShortcutBindings;

#[derive(Debug, Clone)]
pub struct ShortcutBindingsCache {
    bindings: ShortcutBindings,
    has_loaded: bool,
}

impl Default for ShortcutBindingsCache {
    fn default() -> Self {
        Self {
            bindings: ShortcutBindings::defaults(),
            has_loaded: false,
        }
    }
}

impl ShortcutBindingsCache {
    pub fn bindings(&self) -> &ShortcutBindings {
        &self.bindings
    }

    /// 캐시가 저장된 표를 실제로 담고 있는지.
    /// 저장은 표를 통째로 치환하므로, 조회에 실패한 기본값 캐시를 저장하면 다른 명령의 재배정이 지워진다.
    pub fn has_loaded(&self) -> bool {
        self.has_loaded
    }

    /// 저장소 조회 결과를 반영하고 표를 담았는지 돌려준다.
    /// 다시 읽기에 실패하면 캐시는 다른 창이 바꾼 값을 놓친 옛 표이므로, 앞서 성공했더라도 담고 있지 않다고 본다.
    pub fn apply_load_result<E>(&mut self, load_result: Result<ShortcutBindings, E>) -> bool {
        match load_result {
            Ok(bindings) => {
                self.bindings = bindings;
                self.has_loaded = true;
            }
            Err(_) => self.has_loaded = false,
        }
        self.has_loaded
    }

    /// 저장에 성공한 표로 캐시를 바꾼다
    pub fn publish(&mut self, bindings: ShortcutBindings) {
        self.bindings = bindings;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::bindings::ShortcutCommandId;

    #[test]
    fn failed_load_keeps_defaults_and_reports_failure() {
        let mut cache = ShortcutBindingsCache::default();

        assert!(!cache.apply_load_result(Err::<ShortcutBindings, _>("설정 조회 실패")));
        assert!(!cache.has_loaded());
        assert_eq!(cache.bindings(), &ShortcutBindings::defaults());
    }

    #[test]
    fn successful_load_stores_the_saved_table() {
        let mut cache = ShortcutBindingsCache::default();
        let mut stored_bindings = ShortcutBindings::defaults();
        stored_bindings.set_shortcut(ShortcutCommandId::TaskSearch, "Mod+J");

        assert!(cache.apply_load_result(Ok::<_, ()>(stored_bindings.clone())));
        assert!(cache.has_loaded());
        assert_eq!(cache.bindings(), &stored_bindings);
    }

    #[test]
    fn failed_reload_after_success_is_not_loaded() {
        let mut cache = ShortcutBindingsCache::default();
        assert!(cache.apply_load_result(Ok::<_, &str>(ShortcutBindings::defaults())));

        assert!(!cache.apply_load_result(Err::<ShortcutBindings, _>("설정 조회 실패")));
        assert!(!cache.has_loaded());
    }
}
