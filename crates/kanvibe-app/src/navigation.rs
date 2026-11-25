//! 창 하나의 앱 내부 히스토리. TS `useRouter`(`navigation.tsx`)의 back·forward·push·replace 의미를 옮겼다.

use kanvibe_domain::i18n::Locale;

use crate::routes::{LocalizedRoute, RouteResolution, localize_href, resolve_route_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationHistory {
    entries: Vec<String>,
    current_index: usize,
}

impl NavigationHistory {
    /// 창이 처음 여는 경로. 리다이렉트 경로(`/`, 모르는 locale)는 그 자리에서 바꿔 둔다.
    pub fn new(initial_path: &str) -> Self {
        let mut history = Self {
            entries: vec![initial_path.to_string()],
            current_index: 0,
        };
        history.follow_redirects();
        history
    }

    /// 히스토리 여러 개와 현재 위치를 바로 정한다(새 창 복원, 테스트)
    pub fn with_entries(entries: Vec<String>, current_index: usize) -> Self {
        assert!(
            current_index < entries.len(),
            "current index must point at an entry"
        );
        Self {
            entries,
            current_index,
        }
    }

    pub fn current_path(&self) -> &str {
        &self.entries[self.current_index]
    }

    /// 현재 경로가 그릴 화면. 리다이렉트는 히스토리에 남기지 않으므로 항상 `Render`다.
    pub fn current_route(&self) -> LocalizedRoute {
        match resolve_route_path(self.current_path()) {
            RouteResolution::Render(route) => route,
            RouteResolution::Redirect(target) => {
                unreachable!("redirect {target} must be followed before rendering")
            }
        }
    }

    pub fn current_locale(&self) -> Locale {
        self.current_route().locale
    }

    pub fn can_go_back(&self) -> bool {
        self.current_index > 0
    }

    /// 이전 경로로 간다. 앱 안 이전 경로가 없으면 현재 locale의 보드로 바꾸고, 이미 보드면 그대로 둔다.
    pub fn back(&mut self) {
        if self.can_go_back() {
            self.current_index -= 1;
            return;
        }
        let home_path = localize_href("/", Some(self.current_locale().as_str()));
        let current_pathname = self.current_path().split('?').next().unwrap_or_default();
        if current_pathname != home_path {
            self.replace(&home_path);
        }
    }

    pub fn forward(&mut self) {
        if self.current_index + 1 < self.entries.len() {
            self.current_index += 1;
        }
    }

    /// 새 경로를 쌓는다. 앞으로 가기 기록은 버린다.
    pub fn push(&mut self, href: &str) {
        let target = self.localize(href);
        self.entries.truncate(self.current_index + 1);
        self.entries.push(target);
        self.current_index += 1;
        self.follow_redirects();
    }

    /// 히스토리를 늘리지 않고 현재 경로를 바꾼다
    pub fn replace(&mut self, href: &str) {
        let target = self.localize(href);
        self.entries[self.current_index] = target;
        self.follow_redirects();
    }

    fn localize(&self, href: &str) -> String {
        let current_locale = match resolve_route_path(self.current_path()) {
            RouteResolution::Render(route) => route.locale,
            RouteResolution::Redirect(_) => Locale::DEFAULT,
        };
        localize_href(href, Some(current_locale.as_str()))
    }

    fn follow_redirects(&mut self) {
        if let RouteResolution::Redirect(target) = resolve_route_path(self.current_path()) {
            self.entries[self.current_index] = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(entries: &[&str], current_index: usize) -> NavigationHistory {
        NavigationHistory::with_entries(
            entries.iter().map(|entry| entry.to_string()).collect(),
            current_index,
        )
    }

    // parity: TS-renderer-navigation-f1f2ce
    #[test]
    fn back_returns_to_the_previous_page() {
        let mut history = history(&["/ko", "/ko/task/task-1"], 1);
        history.back();
        assert_eq!(history.current_path(), "/ko");
    }

    // parity: TS-renderer-navigation-55a7de
    #[test]
    fn back_without_history_goes_to_the_current_locale_board() {
        let mut history = history(&["/ko/task/task-1"], 0);
        history.back();
        assert_eq!(history.current_path(), "/ko");
    }

    // parity: TS-renderer-navigation-ee5898
    #[test]
    fn back_keeps_the_locale_of_the_current_page() {
        let mut history = history(&["/zh/task/task-1"], 0);
        history.back();
        assert_eq!(history.current_path(), "/zh");
    }

    // parity: TS-renderer-navigation-03a63c
    #[test]
    fn back_on_the_board_without_history_stays() {
        let mut history = history(&["/ko"], 0);
        history.back();
        assert_eq!(history.current_path(), "/ko");
    }

    // parity: TS-renderer-navigation-32913e
    #[test]
    fn only_in_app_history_counts_as_a_previous_page() {
        let mut history = NavigationHistory::new("/en/task/task-1");
        history.back();
        assert_eq!(history.current_path(), "/en");
    }

    // parity: TS-renderer-navigation-56c64e
    #[test]
    fn forward_moves_to_the_next_page() {
        let mut history = history(&["/ko", "/ko/task/task-1", "/ko/task/task-2"], 1);
        history.forward();
        assert_eq!(history.current_path(), "/ko/task/task-2");
    }

    #[test]
    fn push_localizes_and_drops_forward_entries() {
        let mut history = history(&["/en", "/en/settings"], 0);
        history.push("/task/t1");
        assert_eq!(history.current_path(), "/en/task/t1");
        history.forward();
        assert_eq!(history.current_path(), "/en/task/t1");
    }

    #[test]
    fn redirect_paths_are_replaced_instead_of_stacked() {
        assert_eq!(NavigationHistory::new("/").current_path(), "/ko");
        let mut history = NavigationHistory::new("/fr/settings");
        assert_eq!(history.current_path(), "/ko");
        history.push("/fr/task/t1");
        assert_eq!(history.current_path(), "/ko/fr/task/t1");
        history.back();
        assert_eq!(history.current_path(), "/ko");
        assert!(!history.can_go_back());
    }
}
