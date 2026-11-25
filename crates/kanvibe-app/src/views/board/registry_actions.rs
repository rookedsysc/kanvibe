//! 프로젝트 등록 대화상자의 동작(TS `ProjectRegistryDialog`): 경로 스캔·등록, 등록 프로젝트 삭제, 폴더 검색칸 조회.
//! 스캔과 삭제는 진행·실패 상태를 따로 두고, 대화상자를 닫은 뒤 끝난 결과는 토스트로 알린다.

use gpui_kit::{Context, Window};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_services::project::ScanResult;

use super::BoardView;
use super::folder_search::FolderSearch;
use super::text_field::TextFieldEvent;
use crate::app_events::{RefreshScope, trigger_refresh};
use crate::components::toast::ToastKind;
use crate::qa::prompts::confirm_action;

/// 스캔 결과 뒤에 보이는 한 줄 안내
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryNotice {
    pub is_error: bool,
    pub message: String,
}

/// 진행 중이거나 실패한 스캔(다시 시도는 같은 인자로)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ScanAction {
    pub is_pending: bool,
    pub last_request: Option<(String, Option<String>)>,
    pub failure: Option<String>,
    pub result: Option<ScanResult>,
}

/// 삭제 중이거나 실패한 프로젝트: (ID, 이름)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct DeleteAction {
    pub pending: Option<(String, String)>,
    pub failed: Option<(String, String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RegistryState {
    pub folder: FolderSearch,
    pub is_host_menu_open: bool,
    pub scan: ScanAction,
    pub delete: DeleteAction,
    pub notice: Option<RegistryNotice>,
    pub folder_request: Option<(String, Option<String>)>,
    /// 대화상자를 열 때마다 바뀐다. 닫았다 다시 연 대화상자는 이전 요청의 결과를 받지 않는다
    pub generation: u64,
}

/// TS `applyScanResult`: 등록·worktree 수, 없으면 "새 프로젝트 없음", 그것도 없고 오류도 없으면 "git 저장소 없음"
pub fn scan_result_notice(result: &ScanResult, locale: Locale) -> Option<RegistryNotice> {
    let labels = Translator::new(locale, "settings");
    let mut messages = Vec::new();
    if !result.registered.is_empty() {
        let count = result.registered.len().to_string();
        messages.push(labels.translate_with("registeredCount", &[("count", &count)]));
    }
    if !result.worktree_tasks.is_empty() {
        let count = result.worktree_tasks.len().to_string();
        messages.push(labels.translate_with("worktreeTasksRegistered", &[("count", &count)]));
    }
    if !messages.is_empty() {
        return Some(RegistryNotice {
            is_error: false,
            message: messages.join(" / "),
        });
    }
    if !result.skipped.is_empty() {
        return Some(RegistryNotice {
            is_error: false,
            message: labels.translate("noNewProjects"),
        });
    }
    result.errors.is_empty().then(|| RegistryNotice {
        is_error: true,
        message: labels.translate("noGitRepos"),
    })
}

impl BoardView {
    /// 폴더 검색칸 이벤트
    pub(super) fn handle_registry_path_event(
        &mut self,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = &mut self.registry.folder;
        match event {
            TextFieldEvent::Changed(text) => folder.type_input(text.clone()),
            TextFieldEvent::NavigationKey(key) if key == "down" => folder.move_selection(true),
            TextFieldEvent::NavigationKey(key) if key == "up" => folder.move_selection(false),
            TextFieldEvent::NavigationKey(key) if key == "tab" && folder.is_open => {
                folder.enter_selected();
                self.sync_registry_path_field(cx);
            }
            TextFieldEvent::NavigationKey(_) => {}
            TextFieldEvent::Submitted { .. } => {
                if folder.is_open && folder.select_highlighted() {
                    self.sync_registry_path_field(cx);
                } else {
                    self.submit_registry_scan(cx);
                }
            }
            TextFieldEvent::Escaped if folder.is_open => folder.is_open = false,
            TextFieldEvent::Escaped => return self.close_project_registry(window, cx),
        }
        self.request_folder_listing(cx);
        cx.notify();
    }

    /// 입력칸 글자를 폴더 검색 상태와 맞춘다(Tab·선택으로 경로가 바뀐 뒤)
    pub(super) fn sync_registry_path_field(&mut self, cx: &mut Context<Self>) {
        let input = self.registry.folder.input.clone();
        self.registry_path
            .update(cx, |field, cx| field.set_text(input, cx));
        self.request_folder_listing(cx);
    }

    /// 지금 경로·호스트의 하위 폴더를 아직 받지 않았으면 읽는다
    pub(super) fn request_folder_listing(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.registry.folder.pending_request() else {
            return;
        };
        if self.registry.folder_request.as_ref() == Some(&request) {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.registry.folder_request = Some(request.clone());
        let (parent_path, ssh_host) = request;
        let load = services.list_subdirectories(parent_path.clone(), ssh_host.clone(), cx);
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                let request = (parent_path.clone(), ssh_host.clone());
                if this.registry.folder_request.as_ref() == Some(&request) {
                    this.registry.folder_request = None;
                }
                let outcome = result.map_err(|failure| failure.reason);
                this.registry.folder.receive(parent_path, ssh_host, outcome);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn retry_folder_listing(&mut self, cx: &mut Context<Self>) {
        self.registry.folder.retry();
        self.request_folder_listing(cx);
        cx.notify();
    }

    pub(super) fn choose_folder(&mut self, directory: &str, cx: &mut Context<Self>) {
        self.registry.folder.select_directory(directory);
        self.sync_registry_path_field(cx);
        cx.notify();
    }

    pub(super) fn choose_scan_host(&mut self, ssh_host: Option<String>, cx: &mut Context<Self>) {
        self.registry.is_host_menu_open = false;
        self.registry.folder.change_host(ssh_host);
        self.request_folder_listing(cx);
        cx.notify();
    }

    /// 스캔 폼 제출. 검색어를 비워 새로 등록된 프로젝트가 가려지지 않게 한다
    pub(super) fn submit_registry_scan(&mut self, cx: &mut Context<Self>) {
        self.registry.notice = None;
        self.registry.scan.result = None;
        self.registry_search
            .update(cx, |field, cx| field.set_text("", cx));
        let scan_path = self.registry.folder.submitted_path();
        if scan_path.is_empty() {
            self.registry.notice = Some(RegistryNotice {
                is_error: true,
                message: Translator::new(self.locale(), "settings").translate("scanPathRequired"),
            });
            cx.notify();
            return;
        }
        let ssh_host = self.registry.folder.ssh_host().map(str::to_string);
        self.run_registry_scan(scan_path, ssh_host, cx);
    }

    /// 실패한 스캔을 같은 경로·호스트로 다시 돌린다
    pub(super) fn retry_registry_scan(&mut self, cx: &mut Context<Self>) {
        if let Some((scan_path, ssh_host)) = self.registry.scan.last_request.clone() {
            self.run_registry_scan(scan_path, ssh_host, cx);
        }
    }

    fn run_registry_scan(
        &mut self,
        scan_path: String,
        ssh_host: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = self.services.clone() else {
            return;
        };
        if self.registry.scan.is_pending {
            return;
        }
        self.registry.scan.is_pending = true;
        self.registry.scan.failure = None;
        self.registry.scan.last_request = Some((scan_path.clone(), ssh_host.clone()));
        let scan = services.scan_projects(scan_path, ssh_host, cx);
        let generation = self.registry.generation;
        cx.spawn(async move |this, cx| {
            let result = scan.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_registry_scan(generation, result, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_registry_scan(
        &mut self,
        generation: u64,
        result: super::services::BoardServiceResult<ScanResult>,
        cx: &mut Context<Self>,
    ) {
        let is_same_dialog = self.is_registry_dialog(generation);
        if is_same_dialog {
            self.registry.scan.is_pending = false;
        }
        let labels = Translator::new(self.locale(), "settings");
        match result {
            Ok(scan_result) => {
                trigger_refresh(RefreshScope::All, cx);
                let notice = scan_result_notice(&scan_result, self.locale());
                let is_announcement =
                    !scan_result.registered.is_empty() || !scan_result.worktree_tasks.is_empty();
                if !is_same_dialog {
                    if let Some(notice) = notice.filter(|_| is_announcement) {
                        self.push_toast(ToastKind::Success, notice.message, None, cx);
                    }
                    return;
                }
                self.registry.notice = notice;
                self.registry.scan.result = Some(scan_result);
            }
            Err(failure) => {
                if !is_same_dialog {
                    let message =
                        labels.translate_with("scanFailed", &[("reason", &failure.reason)]);
                    self.push_toast(ToastKind::Failure, message, None, cx);
                    return;
                }
                self.registry.scan.failure = Some(failure.reason);
            }
        }
        cx.notify();
    }

    /// 요청을 보낸 대화상자가 아직 열려 있는지
    fn is_registry_dialog(&self, generation: u64) -> bool {
        self.is_registry_open && self.registry.generation == generation
    }

    /// 확인을 받은 뒤 등록 프로젝트를 지운다. 다른 삭제가 진행 중이면 무시한다
    pub(super) fn request_project_deletion(
        &mut self,
        project_id: String,
        project_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.registry.delete.pending.is_some() {
            return;
        }
        let message = Translator::new(self.locale(), "settings")
            .translate_with("deleteConfirm", &[("name", &project_name)]);
        let answer = confirm_action(&message, None, window, cx);
        cx.spawn(async move |this, cx| {
            if !answer.await {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.run_project_deletion(project_id, project_name, cx)
            });
        })
        .detach();
    }

    /// 실패한 삭제를 같은 프로젝트로 다시 시도한다
    pub(super) fn retry_project_deletion(&mut self, cx: &mut Context<Self>) {
        if let Some((project_id, project_name, _)) = self.registry.delete.failed.clone() {
            self.run_project_deletion(project_id, project_name, cx);
        }
    }

    fn run_project_deletion(
        &mut self,
        project_id: String,
        project_name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = self.services.clone() else {
            return;
        };
        if self.registry.delete.pending.is_some() {
            return;
        }
        self.registry.delete = DeleteAction {
            pending: Some((project_id.clone(), project_name.clone())),
            failed: None,
        };
        let deletion = services.delete_project(project_id.clone(), cx);
        let generation = self.registry.generation;
        cx.spawn(async move |this, cx| {
            let result = deletion.await;
            let _ = this.update(cx, |this, cx| {
                let outcome = result.map(|_| ()).map_err(|failure| failure.reason);
                this.finish_project_deletion(generation, project_id, project_name, outcome, cx)
            });
        })
        .detach();
        cx.notify();
    }

    /// 삭제 결과. 대화상자를 닫은 뒤 끝났으면 대화상자 대신 토스트로 알린다
    fn finish_project_deletion(
        &mut self,
        generation: u64,
        project_id: String,
        project_name: String,
        outcome: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let is_same_dialog = self.is_registry_dialog(generation);
        if is_same_dialog {
            self.registry.delete.pending = None;
        }
        let labels = Translator::new(self.locale(), "settings");
        match outcome {
            Ok(()) => {
                trigger_refresh(RefreshScope::All, cx);
                if !is_same_dialog {
                    let message =
                        labels.translate_with("deleteProjectSucceeded", &[("name", &project_name)]);
                    self.push_toast(ToastKind::Success, message, None, cx);
                }
            }
            Err(reason) if is_same_dialog => {
                self.registry.delete.failed = Some((project_id, project_name, reason));
            }
            Err(reason) => {
                let message = labels.translate_with("deleteProjectFailed", &[("reason", &reason)]);
                self.push_toast(ToastKind::Failure, message, None, cx);
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(registered: &[&str], skipped: &[&str], errors: &[&str]) -> ScanResult {
        let strings = |items: &[&str]| items.iter().map(|item| item.to_string()).collect();
        ScanResult {
            registered: strings(registered),
            skipped: strings(skipped),
            errors: strings(errors),
            ..ScanResult::default()
        }
    }

    #[test]
    fn scan_notice_follows_the_ts_priority() {
        let registered = scan_result_notice(&result(&["a", "b"], &[], &[]), Locale::Ko).unwrap();
        assert_eq!(registered.message, "2개 프로젝트 등록 완료");
        assert!(!registered.is_error);
        let skipped = scan_result_notice(&result(&[], &["a"], &[]), Locale::Ko).unwrap();
        assert_eq!(skipped.message, "새로 등록할 프로젝트가 없습니다.");
        let nothing = scan_result_notice(&result(&[], &[], &[]), Locale::Ko).unwrap();
        assert!(nothing.is_error);
        assert_eq!(
            scan_result_notice(&result(&[], &[], &["x"]), Locale::Ko),
            None
        );
    }
}
