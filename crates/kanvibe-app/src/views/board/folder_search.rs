//! 프로젝트 스캔 경로 입력(TS `FolderSearchInput`)의 상태. 입력값의 마지막 `/` 앞은 탐색할 부모 경로, 뒤는 검색어다.
//! 부모 경로·호스트마다 하위 폴더를 읽어 퍼지 검색으로 거르고, ↑↓로 고르고 Tab으로 하위 폴더에 들어가며 Enter로 확정한다.
//! 다른 경로·호스트의 늦은 응답은 지금 목록을 바꾸지 않는다.

use kanvibe_process::project_search::{FuzzyMatch, fuzzy_match};

/// 한 경로·호스트의 조회 결과
#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryListing {
    parent_path: String,
    ssh_host: Option<String>,
    outcome: Result<Vec<String>, String>,
}

/// 드롭다운에 보일 내용
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderListView {
    Loading,
    Failed(String),
    Empty,
    Directories(Vec<FuzzyMatch>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSearch {
    pub input: String,
    selected_path: String,
    pub selected_index: usize,
    pub is_open: bool,
    ssh_host: Option<String>,
    listing: Option<DirectoryListing>,
}

impl Default for FolderSearch {
    fn default() -> Self {
        Self {
            input: "~/".into(),
            selected_path: String::new(),
            selected_index: 0,
            is_open: false,
            ssh_host: None,
            listing: None,
        }
    }
}

impl FolderSearch {
    /// 탐색할 부모 경로와 검색어. `/abc`는 루트 `/`, 슬래시가 없으면 홈 `~`이다
    pub fn parent_and_term(&self) -> (String, String) {
        match self.input.rfind('/') {
            Some(0) => ("/".into(), self.input[1..].to_string()),
            Some(index) => (
                self.input[..index].to_string(),
                self.input[index + 1..].to_string(),
            ),
            None => ("~".into(), String::new()),
        }
    }

    pub fn ssh_host(&self) -> Option<&str> {
        self.ssh_host.as_deref()
    }

    /// 지금 입력이 읽어야 할 (부모 경로, 호스트). 이미 받은 결과가 있으면 `None`이다
    pub fn pending_request(&self) -> Option<(String, Option<String>)> {
        let (parent_path, _) = self.parent_and_term();
        (!self.is_current(self.listing.as_ref())).then(|| (parent_path, self.ssh_host.clone()))
    }

    fn is_current(&self, listing: Option<&DirectoryListing>) -> bool {
        let (parent_path, _) = self.parent_and_term();
        listing.is_some_and(|listing| {
            listing.parent_path == parent_path && listing.ssh_host == self.ssh_host
        })
    }

    /// 사용자가 글자를 바꿨다. 확정했던 선택은 버린다
    pub fn type_input(&mut self, input: String) {
        self.input = input;
        self.selected_path.clear();
        self.selected_index = 0;
        self.is_open = true;
    }

    pub fn change_host(&mut self, ssh_host: Option<String>) {
        self.ssh_host = ssh_host;
        self.selected_index = 0;
    }

    /// 조회 결과를 받는다. 지금 입력·호스트와 다른 요청의 응답이면 버린다
    pub fn receive(
        &mut self,
        parent_path: String,
        ssh_host: Option<String>,
        outcome: Result<Vec<String>, String>,
    ) {
        let listing = DirectoryListing {
            parent_path,
            ssh_host,
            outcome,
        };
        if self.is_current(Some(&listing)) {
            self.listing = Some(listing);
            self.selected_index = 0;
        }
    }

    /// 다시 불러오기: 지금 경로의 실패를 지우고 다시 읽게 한다
    pub fn retry(&mut self) {
        if self.is_current(self.listing.as_ref()) {
            self.listing = None;
        }
    }

    pub fn view(&self) -> FolderListView {
        let Some(listing) = self
            .listing
            .as_ref()
            .filter(|listing| self.is_current(Some(listing)))
        else {
            return FolderListView::Loading;
        };
        let directories = match &listing.outcome {
            Ok(directories) => directories,
            Err(reason) => return FolderListView::Failed(reason.clone()),
        };
        let (_, term) = self.parent_and_term();
        let matches: Vec<FuzzyMatch> = if term.is_empty() {
            directories
                .iter()
                .map(|path| FuzzyMatch {
                    path: path.clone(),
                    score: 0,
                    matched_indices: Vec::new(),
                })
                .collect()
        } else {
            let mut found: Vec<FuzzyMatch> = directories
                .iter()
                .filter_map(|name| fuzzy_match(&term, name))
                .collect();
            found.sort_by_key(|found| std::cmp::Reverse(found.score));
            found
        };
        if matches.is_empty() {
            FolderListView::Empty
        } else {
            FolderListView::Directories(matches)
        }
    }

    fn visible_directories(&self) -> Vec<FuzzyMatch> {
        match self.view() {
            FolderListView::Directories(matches) => matches,
            _ => Vec::new(),
        }
    }

    fn child_path(&self, directory: &str) -> String {
        let (parent_path, _) = self.parent_and_term();
        let prefix = if parent_path == "/" { "" } else { &parent_path };
        format!("{prefix}/{directory}")
    }

    pub fn move_selection(&mut self, is_down: bool) {
        if !self.is_open {
            self.is_open = true;
            return;
        }
        let count = self.visible_directories().len();
        self.selected_index = if is_down {
            (self.selected_index + 1).min(count.saturating_sub(1))
        } else {
            self.selected_index.saturating_sub(1)
        };
    }

    /// Tab: 고른 폴더 안으로 들어간다. 받은 목록이 없으면(로딩 중) 아무것도 하지 않는다
    pub fn enter_selected(&mut self) {
        let Some(directory) = self.visible_directories().get(self.selected_index).cloned() else {
            return;
        };
        self.input = format!("{}/", self.child_path(&directory.path));
        self.selected_index = 0;
    }

    /// 폴더를 확정한다(Enter·클릭)
    pub fn select_directory(&mut self, directory: &str) {
        let path = self.child_path(directory);
        self.selected_path = path.clone();
        self.input = path;
        self.is_open = false;
    }

    /// Enter: 고른 폴더를 확정한다. 받은 목록이 없으면 무시한다
    pub fn select_highlighted(&mut self) -> bool {
        let Some(directory) = self.visible_directories().get(self.selected_index).cloned() else {
            return false;
        };
        self.select_directory(&directory.path);
        true
    }

    /// 스캔에 넘길 경로: 목록에서 확정한 경로가 있으면 그것, 없으면 입력값 그대로
    pub fn submitted_path(&self) -> String {
        if self.selected_path.is_empty() {
            self.input.clone()
        } else {
            self.selected_path.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(search: &mut FolderSearch, directories: &[&str]) {
        let (parent, _) = search.parent_and_term();
        let host = search.ssh_host.clone();
        search.receive(
            parent,
            host,
            Ok(directories.iter().map(|name| name.to_string()).collect()),
        );
    }

    #[test]
    fn a_typed_remote_path_is_submitted_as_is() {
        let mut search = FolderSearch::default();
        search.change_host(Some("devbox".into()));
        search.type_input("/srv/repos".into());
        assert_eq!(search.submitted_path(), "/srv/repos");
    }

    #[test]
    fn a_path_chosen_from_the_list_wins_over_the_input() {
        let mut search = FolderSearch::default();
        ready(&mut search, &["work", "play"]);
        search.is_open = true;
        search.move_selection(true);
        assert!(search.select_highlighted());
        assert_eq!(search.submitted_path(), "~/play");
        assert!(!search.is_open);
    }

    #[test]
    fn late_answers_for_an_old_path_and_keys_while_loading_are_ignored() {
        let mut search = FolderSearch::default();
        search.type_input("~/work/".into());
        search.receive("~".into(), None, Ok(vec!["stale".into()]));
        assert_eq!(search.view(), FolderListView::Loading);
        search.enter_selected();
        assert!(!search.select_highlighted());
        assert_eq!(search.input, "~/work/");
    }

    #[test]
    fn a_failure_from_the_previous_host_keeps_the_current_list() {
        let mut search = FolderSearch::default();
        search.change_host(Some("devbox".into()));
        ready(&mut search, &["repo"]);
        search.receive("~".into(), None, Err("timeout".into()));
        assert!(
            matches!(search.view(), FolderListView::Directories(found) if found[0].path == "repo")
        );
    }

    #[test]
    fn a_success_from_the_previous_host_does_not_end_the_current_loading() {
        let mut search = FolderSearch::default();
        search.change_host(Some("devbox".into()));
        search.receive("~".into(), None, Ok(vec!["local".into()]));
        assert_eq!(search.view(), FolderListView::Loading);
        assert_eq!(
            search.pending_request(),
            Some(("~".into(), Some("devbox".into())))
        );
    }

    #[test]
    fn the_absolute_root_keeps_single_slash_paths_for_click_and_tab() {
        let mut search = FolderSearch::default();
        search.type_input("/".into());
        assert_eq!(search.parent_and_term(), ("/".into(), String::new()));
        ready(&mut search, &["srv", "home"]);
        search.enter_selected();
        assert_eq!(search.input, "/srv/");
        search.type_input("/".into());
        ready(&mut search, &["srv"]);
        search.select_directory("srv");
        assert_eq!(search.submitted_path(), "/srv");
    }

    #[test]
    fn a_failed_listing_shows_the_reason_and_reload_lists_again() {
        let mut search = FolderSearch::default();
        search.receive("~".into(), None, Err("permission denied".into()));
        assert_eq!(
            search.view(),
            FolderListView::Failed("permission denied".into())
        );
        search.retry();
        assert_eq!(search.pending_request(), Some(("~".into(), None)));
        ready(&mut search, &["work"]);
        assert!(matches!(search.view(), FolderListView::Directories(_)));
    }

    #[test]
    fn typing_keeps_working_after_a_listing_failure() {
        let mut search = FolderSearch::default();
        search.receive("~".into(), None, Err("boom".into()));
        search.type_input("~/wo".into());
        assert_eq!(search.input, "~/wo");
        ready(&mut search, &["work", "play"]);
        assert!(matches!(search.view(), FolderListView::Directories(found) if found.len() == 1));
    }
}
