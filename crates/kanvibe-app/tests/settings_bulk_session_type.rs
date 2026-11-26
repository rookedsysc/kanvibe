//! 세션 타입 일괄 변경(TS `BulkSessionTypeChangeSettings`). 작업 서비스 대신 가짜 통로를 두고,
//! 확인 대화상자 답을 정해 둔 채 흐름이 남긴 결과·단계·서비스 호출을 본다.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::gpui;
use gpui_kit::{AsyncApp, TestAppContext};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
use kanvibe_app::services::{ServiceFailure, ServiceResult};
use kanvibe_app::views::settings::bulk_session_type::{
    BulkChangeOutcome, BulkChangeStage, ConfirmPrompt, SessionDependencyStatus,
    SessionTypeBulkChanger, SessionTypeChangeResult, SessionTypeChangeTargets, StageReporter,
    run_bulk_change,
};
use kanvibe_domain::i18n::Locale;
use kanvibe_domain::task::SessionType;

#[derive(Default)]
struct ChangerState {
    targets: Option<SessionTypeChangeTargets>,
    /// 도구가 없는 호스트(설치를 묻는다)
    missing_tool_hosts: Vec<Option<String>>,
    results: VecDeque<SessionTypeChangeResult>,
    change_calls: Vec<(SessionType, bool)>,
    install_calls: usize,
}

#[derive(Default)]
struct FakeChanger {
    state: Mutex<ChangerState>,
}

impl SessionTypeBulkChanger for FakeChanger {
    fn get_session_type_change_targets(
        &self,
        _: SessionType,
    ) -> ServiceResult<SessionTypeChangeTargets> {
        self.state
            .lock()
            .unwrap()
            .targets
            .clone()
            .ok_or(ServiceFailure {
                reason: "no targets configured".into(),
            })
    }

    fn get_session_dependency_status(
        &self,
        _: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        let is_missing = self
            .state
            .lock()
            .unwrap()
            .missing_tool_hosts
            .contains(&ssh_host.map(str::to_string));
        Ok(SessionDependencyStatus {
            available: !is_missing,
            tool_name: Some("herdr".into()),
            is_remote: ssh_host.is_some(),
            ssh_host: ssh_host.map(str::to_string),
            blocked_reason: None,
        })
    }

    fn install_session_dependency(
        &self,
        _: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        self.state.lock().unwrap().install_calls += 1;
        Ok(SessionDependencyStatus {
            available: true,
            tool_name: Some("herdr".into()),
            is_remote: ssh_host.is_some(),
            ssh_host: ssh_host.map(str::to_string),
            blocked_reason: None,
        })
    }

    fn change_all_task_session_types(
        &self,
        session_type: SessionType,
        should_close_existing_sessions: bool,
    ) -> ServiceResult<SessionTypeChangeResult> {
        let mut state = self.state.lock().unwrap();
        state
            .change_calls
            .push((session_type, should_close_existing_sessions));
        Ok(state
            .results
            .pop_front()
            .unwrap_or(SessionTypeChangeResult {
                changed_count: 0,
                failed_count: 0,
            }))
    }
}

/// 확인 대화상자 답을 차례로 돌려주고, 물어본 문구를 남긴다
fn scripted_confirm(answers: Vec<bool>, asked: Rc<RefCell<Vec<String>>>) -> ConfirmPrompt {
    let answers = Rc::new(RefCell::new(VecDeque::from(answers)));
    Box::new(move |message| {
        asked.borrow_mut().push(message);
        let answer = answers.borrow_mut().pop_front().unwrap_or(false);
        Box::pin(async move { answer })
    })
}

fn stage_recorder(stages: Rc<RefCell<Vec<Option<BulkChangeStage>>>>) -> StageReporter {
    Box::new(move |stage, _: &mut AsyncApp| stages.borrow_mut().push(stage))
}

fn init_app(cx: &mut TestAppContext) {
    let launch = LaunchOptions {
        data_dir: None,
        initial_route: "/ko/settings".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: None,
    };
    cx.update(|cx| kanvibe_app::init_app(&launch, cx).expect("app init"));
}

fn targets(
    task_count: usize,
    live_session_count: usize,
    hosts: Vec<Option<String>>,
) -> SessionTypeChangeTargets {
    SessionTypeChangeTargets {
        task_count,
        live_session_count,
        ssh_hosts: hosts,
    }
}

struct Run {
    outcome: Option<BulkChangeOutcome>,
    asked: Vec<String>,
    stages: Vec<Option<BulkChangeStage>>,
}

async fn run(cx: &mut TestAppContext, changer: Arc<FakeChanger>, answers: Vec<bool>) -> Run {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let stages = Rc::new(RefCell::new(Vec::new()));
    let confirm = scripted_confirm(answers, asked.clone());
    let report = stage_recorder(stages.clone());
    let task = cx.spawn(move |mut cx| async move {
        run_bulk_change(
            changer,
            SessionType::Tmux,
            Locale::Ko,
            confirm,
            report,
            &mut cx,
        )
        .await
    });
    cx.run_until_parked();
    let outcome = task.await;
    let asked = asked.borrow().clone();
    let stages = stages.borrow().clone();
    Run {
        outcome,
        asked,
        stages,
    }
}

#[gpui::test]
async fn nothing_to_change_only_informs(cx: &mut TestAppContext) {
    init_app(cx);
    let changer = Arc::new(FakeChanger::default());
    changer.state.lock().unwrap().targets = Some(targets(0, 0, vec![]));
    let run = run(cx, changer.clone(), vec![]).await;
    assert!(run.asked.is_empty(), "묻지 않는다");
    assert!(changer.state.lock().unwrap().change_calls.is_empty());
    assert_eq!(
        run.outcome,
        Some(BulkChangeOutcome {
            message: "바꿀 작업이 없습니다.".into(),
            is_failure: false,
            can_retry: false
        })
    );
}

#[gpui::test]
async fn cancelling_the_count_confirmation_changes_nothing(cx: &mut TestAppContext) {
    init_app(cx);
    let changer = Arc::new(FakeChanger::default());
    changer.state.lock().unwrap().targets = Some(targets(3, 1, vec![None]));
    let run = run(cx, changer.clone(), vec![false]).await;
    assert_eq!(
        run.asked,
        vec!["작업 3개의 세션 타입을 tmux(으)로 바꿀까요?".to_string()]
    );
    assert!(changer.state.lock().unwrap().change_calls.is_empty());
    assert_eq!(run.outcome, None);
}

#[gpui::test]
async fn asks_whether_to_close_live_sessions_and_passes_the_answer(cx: &mut TestAppContext) {
    init_app(cx);
    let changer = Arc::new(FakeChanger::default());
    {
        let mut state = changer.state.lock().unwrap();
        state.targets = Some(targets(3, 2, vec![None]));
        state.results.push_back(SessionTypeChangeResult {
            changed_count: 3,
            failed_count: 0,
        });
    }
    let run = run(cx, changer.clone(), vec![true, true]).await;
    assert_eq!(run.asked.len(), 2);
    assert!(run.asked[1].starts_with("켜져 있는 기존 세션 2개를 닫을까요?"));
    assert_eq!(
        changer.state.lock().unwrap().change_calls,
        vec![(SessionType::Tmux, true)]
    );
    assert_eq!(
        run.outcome.map(|outcome| outcome.message),
        Some("3개를 바꿨습니다. 실패 0개".to_string())
    );
}

#[gpui::test]
async fn without_live_sessions_changes_without_the_close_question(cx: &mut TestAppContext) {
    init_app(cx);
    let changer = Arc::new(FakeChanger::default());
    changer.state.lock().unwrap().targets = Some(targets(2, 0, vec![None]));
    let run = run(cx, changer.clone(), vec![true]).await;
    assert_eq!(run.asked.len(), 1);
    assert_eq!(
        changer.state.lock().unwrap().change_calls,
        vec![(SessionType::Tmux, false)]
    );
}

#[gpui::test]
async fn declining_a_host_tool_install_changes_nothing(cx: &mut TestAppContext) {
    init_app(cx);
    let changer = Arc::new(FakeChanger::default());
    {
        let mut state = changer.state.lock().unwrap();
        state.targets = Some(targets(2, 0, vec![None, Some("remote-a".into())]));
        state.missing_tool_hosts = vec![Some("remote-a".into())];
    }
    let run = run(cx, changer.clone(), vec![true, false]).await;
    assert_eq!(run.asked.len(), 2);
    assert!(run.asked[1].contains("remote-a"));
    let state = changer.state.lock().unwrap();
    assert!(state.change_calls.is_empty());
    assert_eq!(state.install_calls, 0);
    assert_eq!(run.outcome, None);
}

#[gpui::test]
async fn announces_host_preparation_and_change_stages_in_order(cx: &mut TestAppContext) {
    init_app(cx);
    let changer = Arc::new(FakeChanger::default());
    changer.state.lock().unwrap().targets =
        Some(targets(4, 0, vec![None, Some("remote-a".into())]));
    let run = run(cx, changer, vec![true]).await;
    assert_eq!(
        run.stages,
        vec![
            Some(BulkChangeStage::Checking),
            Some(BulkChangeStage::Preparing {
                current: 1,
                total: 2
            }),
            Some(BulkChangeStage::Preparing {
                current: 2,
                total: 2
            }),
            Some(BulkChangeStage::Changing { task_count: 4 }),
            None,
        ]
    );
}

#[gpui::test]
async fn partial_failure_is_reported_and_retry_changes_the_rest(cx: &mut TestAppContext) {
    init_app(cx);
    let changer = Arc::new(FakeChanger::default());
    {
        let mut state = changer.state.lock().unwrap();
        state.targets = Some(targets(3, 0, vec![None]));
        state.results.push_back(SessionTypeChangeResult {
            changed_count: 2,
            failed_count: 1,
        });
        state.results.push_back(SessionTypeChangeResult {
            changed_count: 1,
            failed_count: 0,
        });
    }
    let first = run(cx, changer.clone(), vec![true]).await;
    assert_eq!(
        first.outcome,
        Some(BulkChangeOutcome {
            message: "2개를 바꿨습니다. 실패 1개".into(),
            is_failure: true,
            can_retry: true
        })
    );
    let retry = run(cx, changer.clone(), vec![true]).await;
    assert_eq!(changer.state.lock().unwrap().change_calls.len(), 2);
    assert!(!retry.outcome.unwrap().is_failure);
}
