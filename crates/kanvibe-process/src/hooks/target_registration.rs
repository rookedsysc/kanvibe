//! hook을 설치하는 client를 알림 대상(`.kanvibe/targets.json`)으로 등록하고 등록 여부를 판정한다(TS `hookTargetRegistration.ts`).

use super::client_runtime::{HookInstallContext, HookInstallError};
use crate::git_exclude::ensure_ai_tool_patterns_excluded;
use crate::kanvibe_project_state::{
    HookTargetTransport, KanvibeHookTarget, has_kanvibe_hook_target, parse_kanvibe_targets,
    upsert_kanvibe_hook_target,
};

/// hook 설치 여부는 스크립트에 박힌 TASK_ID가 아니라 targets.json에 현재 client(hook 서버 URL)와 현재 taskId 쌍이
/// 등록되어 있는지로 판정한다. 여러 client가 같은 worktree를 공유해도 서로의 설치 상태를 깨뜨리지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookTargetRegistrationStatus {
    pub has_registered_hook_target: bool,
    pub registered_hook_target_url: Option<String>,
}

fn required_transport(ssh_host: Option<&str>) -> HookTargetTransport {
    match ssh_host {
        Some(_) => HookTargetTransport::Ssh,
        None => HookTargetTransport::Local,
    }
}

/// 같은 client가 다시 설치하면 그 항목이 교체되고, 다른 client는 targets.json에 추가된다.
/// `.kanvibe` 상태 디렉터리는 git 추적에서 뺀다.
pub fn register_kanvibe_hook_target(
    context: &HookInstallContext,
    repo_path: &str,
    task_id: &str,
    hook_server_url: &str,
    ssh_host: Option<&str>,
) -> Result<(), HookInstallError> {
    let ssh_host = ssh_host.filter(|host| !host.is_empty());
    ensure_ai_tool_patterns_excluded(&context.files, repo_path, ssh_host);
    let token = context.runtime.task_cli_token(task_id)?;
    let client_id = context.runtime.hook_client_id()?;
    let remote_host_namespace = match ssh_host {
        Some(host) => context.runtime.remote_host_namespace(host)?,
        None => None,
    };
    let target = KanvibeHookTarget {
        url: hook_server_url.to_string(),
        task_id: task_id.to_string(),
        token: Some(token),
        client_id: Some(client_id),
        transport: Some(required_transport(ssh_host)),
        remote_host_namespace,
    };
    upsert_kanvibe_hook_target(&context.files, repo_path, &target, ssh_host)?;
    if let Some(host) = ssh_host {
        context.runtime.ensure_hook_tunnel(host);
    }
    Ok(())
}

fn has_required_transport(target: Option<&KanvibeHookTarget>, ssh_host: Option<&str>) -> bool {
    target.is_some_and(|target| {
        target.transport == Some(required_transport(ssh_host))
            && (ssh_host.is_none() || target.client_id.as_deref().is_some_and(|id| !id.is_empty()))
    })
}

fn has_token(target: Option<&KanvibeHookTarget>) -> bool {
    target.is_some_and(|target| {
        target
            .token
            .as_deref()
            .is_some_and(|token| !token.is_empty())
    })
}

pub fn verify_hook_target_registration(
    targets_content: &str,
    task_id: Option<&str>,
    expected_hook_server_url: Option<&str>,
    ssh_host: Option<&str>,
) -> HookTargetRegistrationStatus {
    let ssh_host = ssh_host.filter(|host| !host.is_empty());
    let Some(task_id) = task_id.filter(|task_id| !task_id.is_empty()) else {
        return HookTargetRegistrationStatus {
            has_registered_hook_target: true,
            registered_hook_target_url: None,
        };
    };
    let registered_targets = parse_kanvibe_targets(targets_content).targets;
    let registered_target = registered_targets
        .iter()
        .find(|target| target.task_id == task_id);
    let registered_hook_target_url = registered_target.map(|target| target.url.clone());

    // 원격 hook 서버 주소를 확정하지 못한 경우에는 taskId 등록 여부만으로 판정한다
    let Some(expected_url) = expected_hook_server_url.filter(|url| !url.is_empty()) else {
        return HookTargetRegistrationStatus {
            has_registered_hook_target: has_token(registered_target)
                && has_required_transport(registered_target, ssh_host),
            registered_hook_target_url,
        };
    };

    // task CLI 토큰이 없는 예전 등록은 CLI가 쓸 수 없으므로 재설치가 필요한 상태로 본다
    let own_url = expected_url.trim_end_matches('/');
    let own_target = registered_targets
        .iter()
        .find(|target| target.url == own_url);
    let expected_target = KanvibeHookTarget {
        url: expected_url.to_string(),
        task_id: task_id.to_string(),
        ..KanvibeHookTarget::default()
    };
    HookTargetRegistrationStatus {
        has_registered_hook_target: has_kanvibe_hook_target(targets_content, &expected_target)
            && has_token(own_target)
            && has_required_transport(own_target, ssh_host),
        registered_hook_target_url,
    }
}
