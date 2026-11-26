//! 작업 상세 패널 테스트가 함께 쓰는 하네스. 실제 DB와 런타임으로 앱을 띄우고, 화면이 그린 글자·역할로 요소를 찾는다.
//! QA 모드로 띄우면 고장 주입(값 대체·실패·멈춤)을 쓸 수 있고 확인 대화상자를 자동으로 받아들인다.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, WeakEntity};
use kanvibe_app::qa::QaRuntime;
use kanvibe_app::qa::faults::{FaultMode, ServiceFault};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::qa::targets::{DescribedTarget, QaTargetRegistry, TargetQuery};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::AppServices;
use kanvibe_app::shortcuts::ShortcutState;
use kanvibe_app::views::task_detail::TaskDetailView;
use kanvibe_app::views::{ScreenContext, WindowNavigator};
use kanvibe_db::Database;
use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_db::repositories::tasks::{NewKanbanTask, insert_task};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use serde_json::Value;

pub const FIXED_NOW_MS: i64 = 1_768_445_820_000;

/// 테스트 앱 데이터 디렉터리. 앱이 테스트 끝까지 DB를 쥐고 있어 호출부가 `keep()`으로 남기므로,
/// 공용 `/tmp`가 아니라 cargo 대상 폴더 안에 만든다
pub fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("temp dir")
}

pub fn init_app(cx: &mut TestAppContext, data_dir: PathBuf) -> Arc<Database> {
    let launch = LaunchOptions {
        data_dir: Some(data_dir),
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: FIXED_NOW_MS,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
            loading_holds: Default::default(),
        }),
    };
    cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
        cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Linux;
        if !cx.has_global::<QaTargetRegistry>() {
            cx.set_global(QaTargetRegistry::default());
        }
        cx.global::<AppServices>()
            .database()
            .expect("database")
            .clone()
    })
}

pub fn project(database: &Database, name: &str) -> Project {
    project_with_color(database, name, None)
}

pub fn project_with_color(database: &Database, name: &str, color: Option<&str>) -> Project {
    insert_project(
        &database.connection(),
        &NewProject {
            name: name.into(),
            repo_path: format!("/nonexistent/repos/{name}"),
            default_branch: "main".into(),
            ssh_host: None,
            is_worktree: false,
            color: color.map(str::to_string),
            icon_data_url: None,
        },
    )
    .expect("insert project")
}

pub fn task(database: &Database, title: &str, edit: impl FnOnce(&mut NewKanbanTask)) -> KanbanTask {
    let mut new_task = NewKanbanTask {
        title: title.into(),
        description: None,
        status: TaskStatus::Progress,
        branch_name: None,
        worktree_path: None,
        session_type: None,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
    };
    edit(&mut new_task);
    insert_task(&database.connection(), &new_task).expect("insert task")
}

/// 작업을 terminal 세션으로 두어 멀티플렉서 세션을 만들지 않게 한다. terminal 세션도 화면이 연결되면
/// 로컬 셸 PTY를 띄우므로, 테스트 실행기 밖에서 깨어나는 PTY 스레드를 막는 것은 이 함수가 아니라
/// `open_detail`이 터미널 연결(`kanvibe:terminal-open`)을 멈춰 두는 쪽이다
pub fn without_terminal_process(task: &mut NewKanbanTask) {
    task.session_type = Some(kanvibe_domain::task::SessionType::Terminal);
    task.session_name = Some("kanvibe-panels-test".into());
}

/// 화면을 열기 전에 서비스 하나가 끝나지 않게 한다. 자동 연결(`kanban.connectTerminalSession`)을 멈추면
/// 프로젝트 작업을 열어도 테스트 스케줄러 밖에서 깨어나는 PTY 스레드가 생기지 않는다
pub fn hang_before_open(cx: &mut TestAppContext, target: &str) {
    let fault = ServiceFault {
        target: target.into(),
        mode: FaultMode::Hang,
        message: None,
        ms: None,
        value: None,
        after_calls: None,
        argument_match: None,
    };
    cx.update(|cx| {
        cx.global::<AppServices>()
            .faults()
            .expect("qa faults")
            .add(fault);
    });
}

pub struct Detail {
    pub view: Entity<TaskDetailView>,
    pub cx: VisualTestContext,
}

pub fn open_detail(cx: &mut TestAppContext, task_id: &str) -> Detail {
    // 브랜치가 있는 작업은 열자마자 PR 자동 조회가 `gh`를 실행하는 스레드를 띄워 테스트 스케줄러를 깨므로 멈춰 둔다.
    // 패널 테스트는 PR 조회를 보지 않는다
    hang_before_open(cx, "kanban.fetchAndSavePrUrl");
    // 터미널 화면이 연결되면 terminal 세션도 로컬 셸 PTY를 띄우고, PTY 출력 스레드가 테스트 스케줄러 밖에서 화면 수신 작업을
    // 깨운다. 붙은 뒤에는 hook 확인 스레드가 가짜 저장소 경로에 실제 설치를 재시도한다. 패널 테스트는 터미널을 보지 않으므로
    // TS 테스트가 이 경계를 모의했듯 연결을 멈춰 둔다
    hang_before_open(cx, "kanvibe:terminal-open");
    let path = format!("/ko/task/{task_id}");
    let RouteResolution::Render(route) = resolve_route_path(&path) else {
        panic!("{path} must render");
    };
    let (view, visual) = cx.add_window_view(|_, cx| {
        TaskDetailView::new(
            ScreenContext {
                route,
                navigator: WindowNavigator::new(WeakEntity::new_invalid()),
            },
            cx,
        )
    });
    let mut detail = Detail {
        view,
        cx: visual.clone(),
    };
    // 앱과 QA 재생은 연 창을 앞으로 가져온다(`activate_window`). 사용량 자동 조회는 창이 활성일 때만 돈다
    detail.cx.update(|window, _| window.activate_window());
    detail.draw();
    detail
}

pub fn role_name(role: &str, name: &str) -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        role: Some(role.into()),
        name: Some(name.into()),
        exact: Some(true),
        ..DescribedTarget::default()
    })
}

pub fn text_contains(text: &str) -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        text: Some(text.into()),
        exact: Some(false),
        ..DescribedTarget::default()
    })
}

impl Detail {
    /// 지난 프레임 대상을 지우고 다시 그린다
    pub fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| {
            let window_id = window.window_handle().window_id();
            cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
            window.refresh();
        });
        self.cx.run_until_parked();
    }

    pub fn find(&mut self, query: TargetQuery) -> Option<Bounds<Pixels>> {
        self.cx.update(|window, cx| {
            cx.global::<QaTargetRegistry>()
                .find(window.window_handle().window_id(), &query)
        })
    }

    pub fn has_text(&mut self, text: &str) -> bool {
        self.find(TargetQuery::ExactText(text.into())).is_some()
    }

    pub fn has_text_containing(&mut self, text: &str) -> bool {
        self.find(text_contains(text)).is_some()
    }

    pub fn has_role_name(&mut self, role: &str, name: &str) -> bool {
        self.find(role_name(role, name)).is_some()
    }

    pub fn click(&mut self, query: TargetQuery) {
        let bounds = self
            .find(query.clone())
            .unwrap_or_else(|| panic!("{query:?} not found"));
        self.cx
            .simulate_click(bounds.center(), Modifiers::default());
        self.draw();
    }

    pub fn click_text(&mut self, text: &str) {
        self.click(TargetQuery::ExactText(text.into()));
    }

    pub fn click_role(&mut self, role: &str, name: &str) {
        self.click(role_name(role, name));
    }

    pub fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
        self.draw();
    }

    pub fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.draw();
    }

    pub fn fault(
        &mut self,
        target: &str,
        mode: FaultMode,
        message: Option<&str>,
        value: Option<Value>,
    ) {
        self.fault_after(target, mode, message, value, None);
    }

    /// `after_calls`번째 호출 뒤부터 적용하는 고장
    pub fn fault_after(
        &mut self,
        target: &str,
        mode: FaultMode,
        message: Option<&str>,
        value: Option<Value>,
        after_calls: Option<u32>,
    ) {
        let fault = ServiceFault {
            target: target.into(),
            mode,
            message: message.map(str::to_string),
            ms: None,
            value,
            after_calls,
            argument_match: None,
        };
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .faults()
                .expect("qa faults")
                .add(fault);
        });
    }

    /// 이 서비스가 정상 처리 대신 `value`를 돌려주게 한다
    pub fn substitute(&mut self, target: &str, value: Value) {
        self.fault(target, FaultMode::Result, None, Some(value));
    }

    pub fn fail(&mut self, target: &str, message: &str) {
        self.fault(target, FaultMode::Fail, Some(message), None);
    }

    pub fn hang(&mut self, target: &str) {
        self.fault(target, FaultMode::Hang, None, None);
    }

    pub fn clear_fault(&mut self, target: &str) {
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .faults()
                .expect("qa faults")
                .clear(target);
        });
    }

    /// 확인 대화상자를 자동으로 받아들이지 않고 묻게 한다(TS 테스트의 `window.confirm` 모의)
    pub fn ask_confirmations(&mut self) {
        self.cx.update(|_, cx| {
            cx.remove_global::<QaRuntime>();
        });
    }

    pub fn answer_prompt(&mut self, button: &str) {
        self.cx.simulate_prompt_answer(button);
        self.draw();
    }

    pub fn has_pending_prompt(&mut self) -> bool {
        self.cx.has_pending_prompt()
    }

    /// 호출 인자 JSON에 `needle`이 있을 때만 값 대체
    pub fn substitute_matching(&mut self, target: &str, needle: &str, value: Value) {
        let fault = ServiceFault {
            target: target.into(),
            mode: FaultMode::Result,
            message: None,
            ms: None,
            value: Some(value),
            after_calls: None,
            argument_match: Some(needle.into()),
        };
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .faults()
                .expect("qa faults")
                .add(fault);
        });
    }

    /// `ms` 뒤 정상 처리(가상 시계를 그만큼 넘기면 끝난다)
    pub fn delay(&mut self, target: &str, ms: u64) {
        let fault = ServiceFault {
            target: target.into(),
            mode: FaultMode::Delay,
            message: None,
            ms: Some(ms),
            value: None,
            after_calls: None,
            argument_match: None,
        };
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .faults()
                .expect("qa faults")
                .add(fault);
        });
    }

    pub fn advance_clock(&mut self, duration: std::time::Duration) {
        self.cx.executor().advance_clock(duration);
        self.draw();
    }

    pub fn database(&mut self) -> Arc<Database> {
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .database()
                .expect("database")
                .clone()
        })
    }
}

/// 받은 작업 상세 새로 고침 신호 수(TS `router.refresh()` 호출 수)
pub struct RefreshProbe {
    pub refresh_count: usize,
    _subscription: Option<gpui_kit::Subscription>,
}

impl RefreshProbe {
    pub fn new(cx: &mut gpui_kit::Context<Self>) -> Self {
        let subscription = kanvibe_app::app_events::subscribe_to_refresh(
            &[kanvibe_app::app_events::RefreshScope::TaskDetail],
            cx,
            |probe, _| probe.refresh_count += 1,
        );
        Self {
            refresh_count: 0,
            _subscription: subscription,
        }
    }
}

impl Detail {
    pub fn refresh_probe(&mut self) -> Entity<RefreshProbe> {
        use gpui_kit::AppContext;
        self.cx.update(|_, cx| cx.new(RefreshProbe::new))
    }

    pub fn refresh_count(&mut self, probe: &Entity<RefreshProbe>) -> usize {
        self.cx.update(|_, cx| probe.read(cx).refresh_count)
    }

    pub fn has_color_button(&mut self, color: &str) -> bool {
        self.find(TargetQuery::Described(DescribedTarget {
            role: Some("button".into()),
            name: Some("Pick color".into()),
            text: Some(color.into()),
            exact: Some(true),
            ..DescribedTarget::default()
        }))
        .is_some()
    }

    /// 설명 편집 칸이 열려 있고 그 값이 `value`인지
    pub fn has_textbox(&mut self, value: &str) -> bool {
        self.find(TargetQuery::Described(DescribedTarget {
            role: Some("textbox".into()),
            text: Some(value.into()),
            exact: Some(true),
            ..DescribedTarget::default()
        }))
        .is_some()
    }

    pub fn has_any_textbox(&mut self) -> bool {
        self.find(TargetQuery::Described(DescribedTarget {
            role: Some("textbox".into()),
            ..DescribedTarget::default()
        }))
        .is_some()
    }
}
