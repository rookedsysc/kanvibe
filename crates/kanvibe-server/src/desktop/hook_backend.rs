//! hook 경로를 실제 서비스(hook 상태, task CLI, hook 설치 표)에 잇는다(TS `hookService`, `taskCliService`, `hookInstallBundle`).

use std::collections::HashSet;
use std::sync::Arc;

use kanvibe_ai::javascript::js_string;
use kanvibe_process::task_cli::service::{TaskCliError, TaskCliService};
use kanvibe_runtime::DesktopRuntime;
use kanvibe_services::hook_status::HookStatusInput;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::routes::hooks::{HookBackend, TaskCliFailure, TaskCliRoute};

pub struct DesktopHookBackend {
    runtime: Arc<DesktopRuntime>,
}

impl DesktopHookBackend {
    pub fn new(runtime: Arc<DesktopRuntime>) -> Self {
        Self { runtime }
    }
}

/// hook 본문의 `taskId`·`status`. TS는 값이 거짓이면 빠진 것으로 보고, 문자열이 아니면 문자열로 바꿔 썼다
fn read_hook_text(body: &Value, name: &str) -> String {
    match body.get(name) {
        None | Some(Value::Null | Value::Bool(false)) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) if number.as_f64() == Some(0.0) => String::new(),
        Some(value) => js_string(value),
    }
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, TaskCliFailure> {
    serde_json::to_value(value).map_err(|error| TaskCliFailure::Internal(error.to_string()))
}

impl HookBackend for DesktopHookBackend {
    fn update_hook_task_status(
        &self,
        body: &Value,
        authorization: Option<&str>,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> (u16, Value) {
        let input = HookStatusInput {
            task_id: read_hook_text(body, "taskId"),
            status: read_hook_text(body, "status"),
            agent_session: body.get("agentSession").cloned(),
        };
        match self.runtime.hook_status().update_hook_task_status(
            &input,
            authorization,
            source_ssh_hosts,
        ) {
            Ok(updated) => (200, json!({ "success": true, "data": updated })),
            Err(rejection) => (
                rejection.status_code,
                json!({ "success": false, "error": rejection.message, "status": rejection.status_code }),
            ),
        }
    }

    fn run_task_cli(
        &self,
        route: TaskCliRoute,
        authorization: Option<&str>,
        body: &Map<String, Value>,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<Value, TaskCliFailure> {
        let backend = self.runtime.task_cli_backend().clone();
        let service = TaskCliService::new(backend.as_ref());
        let hosts = source_ssh_hosts;
        let outcome = match route {
            TaskCliRoute::Create => service
                .create_task_from_cli(authorization, body, hosts)
                .map(|data| to_value(&data)),
            TaskCliRoute::Delete => service
                .delete_task_from_cli(authorization, body, hosts)
                .map(|data| to_value(&data)),
            TaskCliRoute::Register => service
                .register_task_from_peer(authorization, body, hosts)
                .map(|data| to_value(&data)),
            TaskCliRoute::List => service
                .list_tasks_from_cli(authorization, body, hosts)
                .map(|data| to_value(&data)),
            TaskCliRoute::Session => service
                .prepare_task_session_from_cli(authorization, body, hosts)
                .map(|data| to_value(&data)),
        };
        match outcome {
            Ok(value) => value,
            Err(TaskCliError::Request {
                message,
                status_code,
            }) => Err(TaskCliFailure::Request {
                status: status_code,
                message,
            }),
            Err(TaskCliError::Failed(message)) => Err(TaskCliFailure::Internal(message)),
        }
    }

    fn read_hook_install_script(&self, token: Option<&str>) -> Option<String> {
        self.runtime.hook_install_scripts().read(token)
    }
}
