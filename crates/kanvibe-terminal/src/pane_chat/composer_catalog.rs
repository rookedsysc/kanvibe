//! pane 채팅 입력창의 skill·명령·파일 제안(TS `paneChatComposerCatalog.ts`).
//! CLI가 실제로 읽는 설정 폴더(프로젝트 → HOME → CLI 설정 홈 순서)를 훑어, 같은 이름이면 앞 폴더의 항목을 쓴다.

mod catalog_files;
mod skill_metadata;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kanvibe_ai::javascript::is_js_whitespace;
use kanvibe_ai::node_path;
use kanvibe_domain::pane_chat::{
    FilteredPaneChatSuggestions, MAX_PANE_CHAT_SUGGESTIONS, PaneChatSuggestion,
    filter_pane_chat_suggestions,
};
use kanvibe_process::exec::{ExecGitError, ExecGitOptions};
use kanvibe_process::host_file_access::{HostFileAccess, HostFileError};
use kanvibe_process::worktree::quote_for_posix_shell;
use serde::{Deserialize, Serialize};

use catalog_files::{
    CATALOG_TIMEOUT, CatalogFile, CatalogFiles, CatalogRoot, read_local_catalog_files,
    read_remote_catalog_files,
};
use skill_metadata::read_skill_metadata;

/// 새로 설치한 skill이 메뉴에 나타나기까지 걸릴 수 있는 최대 시간이기도 하다
const COMMAND_CATALOG_TTL: Duration = Duration::from_secs(10);
const MAX_CACHED_COMMAND_CATALOGS: usize = 32;
/// 목록에 없어도 CLI가 늘 받는 기본 명령
const BUILT_IN_COMMANDS: [(&str, &str); 2] = [("model", "모델 선택"), ("help", "CLI 도움말")];

/// 제안을 만들 수 있는 CLI. 다른 CLI는 서비스가 빈 목록을 돌려준다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatCatalogProvider {
    Claude,
    Codex,
}

impl PaneChatCatalogProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatSuggestionKind {
    Skill,
    Command,
    File,
}

/// 제안을 찾을 CLI의 위치. 비어 있는 선택 값은 실행 중인 CLI 프로세스 환경이나 호스트 HOME에서 찾는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatCatalogContext {
    pub provider: PaneChatCatalogProvider,
    pub cwd: String,
    pub ssh_host: Option<String>,
    pub home_directory: Option<String>,
    pub provider_home: Option<String>,
    pub process_id: Option<i64>,
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaneChatCatalogError {
    #[error(transparent)]
    Exec(#[from] ExecGitError),
    /// 원격 HOME을 확인하지 못했다
    #[error("{0}")]
    HomeDirectory(String),
}

impl From<HostFileError> for PaneChatCatalogError {
    fn from(error: HostFileError) -> Self {
        match error {
            HostFileError::Exec(error) => Self::Exec(error),
            other => Self::HomeDirectory(other.to_string()),
        }
    }
}

/// 검색어로 거르기 전의 전체 제안. skill 메뉴와 명령 메뉴는 같은 이름이 겹칠 때 고르는 항목이 달라 따로 둔다
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandCatalog {
    skill: Vec<PaneChatSuggestion>,
    command: Vec<PaneChatSuggestion>,
    is_truncated: bool,
}

type CatalogOutcome = Result<Arc<CommandCatalog>, PaneChatCatalogError>;

/// 진행 중이거나 끝난 훑기 하나. 훑는 중에 온 요청은 같은 결과를 기다린다
#[derive(Default)]
struct CatalogScan {
    outcome: Mutex<Option<CatalogOutcome>>,
    finished: Condvar,
}

impl CatalogScan {
    fn finish(&self, outcome: CatalogOutcome) {
        *self
            .outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(outcome);
        self.finished.notify_all();
    }

    fn wait(&self) -> CatalogOutcome {
        let guard = self
            .outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let finished = self
            .finished
            .wait_while(guard, |outcome| outcome.is_none())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        finished
            .clone()
            .expect("wait_while가 결과가 생길 때까지 기다렸다")
    }
}

struct CachedCatalog {
    key: String,
    read_at: Instant,
    scan: Arc<CatalogScan>,
}

struct ProviderRoots {
    home: String,
    provider: String,
}

/// 앱이 하나를 만들어 pane 채팅 서비스가 쓴다. 최근 훑은 목록을 기억하는 캐시를 함께 가진다
pub struct PaneChatComposerCatalog {
    host_files: Arc<HostFileAccess>,
    /// 오래된 것부터 새것 순서. 다시 훑으면 맨 뒤로 옮긴다
    command_catalogs: Mutex<Vec<CachedCatalog>>,
}

fn file_mention(file_path: &str) -> String {
    let needs_quotes = file_path
        .chars()
        .any(|character| is_js_whitespace(character) || matches!(character, '@' | '"' | '\''));
    if !needs_quotes {
        return format!("@{file_path}");
    }
    let escaped = file_path.replace('\\', "\\\\").replace('"', "\\\"");
    format!("@\"{escaped}\"")
}

/// `ps eww` 출력의 앞이나 공백 뒤에서 `key=`가 시작하는 마지막 자리. 값이 시작하는 위치를 돌려준다
fn last_variable_start(output: &str, key: &str) -> Option<usize> {
    let assignment = format!("{key}=");
    let after_whitespace = output.char_indices().filter_map(|(index, character)| {
        let next = index + character.len_utf8();
        (is_js_whitespace(character) && output[next..].starts_with(&assignment)).then_some(next)
    });
    let at_start = output.starts_with(&assignment).then_some(0);
    at_start
        .into_iter()
        .chain(after_whitespace)
        .last()
        .map(|start| start + assignment.len())
}

/// 공백 뒤에 `NAME=`이 오는 자리. 다음 환경 변수가 시작하는 곳이다
fn is_next_assignment(rest: &str) -> bool {
    let mut characters = rest.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !is_js_whitespace(first) {
        return false;
    }
    let name_and_rest = &rest[first.len_utf8()..];
    let name_length = name_and_rest
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    let starts_with_letter = name_and_rest
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
    starts_with_letter && name_and_rest[name_length..].starts_with('=')
}

/// TS `(?:^|\s)KEY=([\s\S]*?)(?=\s[A-Za-z_][A-Za-z0-9_]*=|$)`의 마지막 일치. 절대 경로가 아니면 버린다
fn read_process_variable(output: &str, key: &str) -> Option<String> {
    let value_start = last_variable_start(output, key)?;
    let value_end = output[value_start..]
        .char_indices()
        .map(|(offset, _)| value_start + offset)
        .find(|index| is_next_assignment(&output[*index..]))
        .unwrap_or(output.len());
    let candidate = &output[value_start..value_end];
    let is_safe_absolute_path =
        node_path::is_absolute(candidate) && !candidate.contains(['\r', '\n', '\0']);
    is_safe_absolute_path.then(|| candidate.to_string())
}

fn is_present(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// 같은 맥락의 요청이 같은 캐시 칸을 쓰도록 맥락 전체를 키로 만든다
fn catalog_key(context: &PaneChatCatalogContext) -> String {
    serde_json::json!([
        context.ssh_host,
        context.provider.as_str(),
        context.cwd,
        context.workspace_path,
        context.process_id,
        context.home_directory,
        context.provider_home,
    ])
    .to_string()
}

/// cwd에서 workspace까지 올라가는 프로젝트 폴더들. 가까운 폴더가 앞이다
fn project_paths(context: &PaneChatCatalogContext) -> Vec<String> {
    let mut paths = vec![context.cwd.clone()];
    let Some(workspace_path) = context
        .workspace_path
        .as_deref()
        .filter(|path| !path.is_empty())
    else {
        return paths;
    };
    let workspace_prefix = format!("{workspace_path}/");
    if context.cwd.starts_with(&workspace_prefix) {
        let mut ancestor = node_path::dirname(&context.cwd);
        while ancestor.starts_with(&workspace_prefix) {
            let parent = node_path::dirname(&ancestor);
            paths.push(ancestor);
            ancestor = parent;
        }
    }
    if !paths.iter().any(|path| path == workspace_path) {
        paths.push(workspace_path.to_string());
    }
    paths
}

fn catalog_root(directory: String, is_command_root: bool) -> CatalogRoot {
    CatalogRoot {
        directory,
        is_command_root,
    }
}

fn catalog_roots(
    provider: PaneChatCatalogProvider,
    projects: &[String],
    roots: &ProviderRoots,
) -> Vec<CatalogRoot> {
    let is_claude = provider == PaneChatCatalogProvider::Claude;
    let mut catalog_roots = Vec::new();
    for project in projects {
        let skills = if is_claude {
            ".claude/skills"
        } else {
            ".agents/skills"
        };
        catalog_roots.push(catalog_root(node_path::join(&[project, skills]), false));
        catalog_roots.push(if is_claude {
            catalog_root(node_path::join(&[project, ".claude/commands"]), true)
        } else {
            catalog_root(node_path::join(&[project, ".codex/skills"]), false)
        });
    }
    let commands = if is_claude { "commands" } else { "prompts" };
    catalog_roots.extend([
        catalog_root(node_path::join(&[&roots.home, ".agents/skills"]), false),
        catalog_root(node_path::join(&[&roots.provider, "skills"]), false),
        catalog_root(node_path::join(&[&roots.provider, commands]), true),
        catalog_root(node_path::join(&[&roots.provider, "plugins/cache"]), false),
    ]);
    catalog_roots
}

/// 명령 파일은 폴더 안 상대 경로를 `:`로 이은 이름을, skill은 SKILL.md가 든 폴더 이름을 기본 이름으로 쓴다
fn fallback_command_name(entry: &CatalogFile) -> String {
    if !entry.root.is_command_root {
        return node_path::basename(&node_path::dirname(&entry.file_path));
    }
    let directory_prefix = format!("{}/", entry.root.directory);
    let relative_path = entry
        .file_path
        .strip_prefix(&directory_prefix)
        .unwrap_or(&entry.file_path);
    let without_extension = relative_path.strip_suffix(".md").unwrap_or(relative_path);
    without_extension.replace('/', ":")
}

fn command_suggestion(
    provider: PaneChatCatalogProvider,
    entry: &CatalogFile,
    name: String,
    description: String,
) -> PaneChatSuggestion {
    let is_command_root = entry.root.is_command_root;
    let prefix = if is_command_root || provider == PaneChatCatalogProvider::Claude {
        "/"
    } else {
        "$"
    };
    let command_name = if is_command_root && provider == PaneChatCatalogProvider::Codex {
        format!("prompts:{name}")
    } else {
        name.clone()
    };
    PaneChatSuggestion {
        id: format!(
            "{}:{name}",
            if is_command_root { "command" } else { "skill" }
        ),
        label: name,
        description,
        insert_text: format!("{prefix}{command_name}"),
    }
}

fn list_command_suggestions(
    provider: PaneChatCatalogProvider,
    files: &[CatalogFile],
    kind: PaneChatSuggestionKind,
) -> Vec<PaneChatSuggestion> {
    let mut seen_names = HashSet::new();
    let mut suggestions = Vec::new();
    for entry in files {
        if kind == PaneChatSuggestionKind::Skill && entry.root.is_command_root {
            continue;
        }
        let Some(metadata) = read_skill_metadata(&entry.content, &fallback_command_name(entry))
        else {
            continue;
        };
        if seen_names.insert(metadata.name.clone()) {
            suggestions.push(command_suggestion(
                provider,
                entry,
                metadata.name,
                metadata.description,
            ));
        }
    }
    if kind == PaneChatSuggestionKind::Command {
        for (command, description) in BUILT_IN_COMMANDS {
            if seen_names.insert(command.to_string()) {
                suggestions.push(PaneChatSuggestion {
                    id: format!("command:{command}"),
                    label: command.to_string(),
                    description: description.to_string(),
                    insert_text: format!("/{command}"),
                });
            }
        }
    }
    suggestions
}

/// 파일 이름이 정렬될 JS 기본 `sort()` 순서(UTF-16 코드 단위)
fn compare_utf16(left: &str, right: &str) -> std::cmp::Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn has_control_character(file: &str) -> bool {
    file.chars()
        .any(|character| character <= '\u{1f}' || character == '\u{7f}')
}

impl PaneChatComposerCatalog {
    pub fn new(host_files: Arc<HostFileAccess>) -> Self {
        Self {
            host_files,
            command_catalogs: Mutex::new(Vec::new()),
        }
    }

    fn command_catalogs(&self) -> MutexGuard<'_, Vec<CachedCatalog>> {
        self.command_catalogs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `kind`에 맞는 제안을 검색어로 거른다. 파일은 늘 20개까지, skill·명령은 `limit`개까지 돌려준다
    pub fn read_pane_chat_suggestions(
        &self,
        context: &PaneChatCatalogContext,
        kind: PaneChatSuggestionKind,
        query: &str,
        limit: usize,
    ) -> Result<FilteredPaneChatSuggestions, PaneChatCatalogError> {
        if kind == PaneChatSuggestionKind::File {
            return self.read_file_suggestions(context, query);
        }
        let catalog = self.read_cached_command_catalog(context)?;
        let suggestions = if kind == PaneChatSuggestionKind::Skill {
            &catalog.skill
        } else {
            &catalog.command
        };
        let matches = filter_pane_chat_suggestions(suggestions, query, limit);
        Ok(FilteredPaneChatSuggestions {
            items: matches.items,
            is_truncated: catalog.is_truncated || matches.is_truncated,
        })
    }

    fn read_file_suggestions(
        &self,
        context: &PaneChatCatalogContext,
        query: &str,
    ) -> Result<FilteredPaneChatSuggestions, PaneChatCatalogError> {
        let workspace = context.workspace_path.as_deref().unwrap_or(&context.cwd);
        // 맨 앞 NUL이 있어야 실행기가 앞뒤 공백을 떼도 공백으로 시작하는 첫 파일 이름이 남는다
        let command = format!(
            "printf '\\0' && git -C {} ls-files --cached --others --exclude-standard -z",
            quote_for_posix_shell(workspace)
        );
        let options = ExecGitOptions {
            timeout: Some(CATALOG_TIMEOUT),
            ..ExecGitOptions::default()
        };
        let output =
            self.host_files
                .runner()
                .exec_git(&command, context.ssh_host.as_deref(), &options)?;
        let lowered_query = query.to_lowercase();
        let mut seen_files = HashSet::new();
        let mut matches: Vec<&str> = output
            .split('\0')
            .filter(|file| !file.is_empty() && !has_control_character(file))
            .filter(|file| file.to_lowercase().contains(&lowered_query))
            .filter(|file| seen_files.insert(*file))
            .collect();
        matches.sort_by(|left, right| compare_utf16(left, right));
        let is_truncated = matches.len() > MAX_PANE_CHAT_SUGGESTIONS;
        let items = matches
            .into_iter()
            .take(MAX_PANE_CHAT_SUGGESTIONS)
            .map(|file| PaneChatSuggestion {
                id: format!("file:{file}"),
                label: file.to_string(),
                description: String::new(),
                insert_text: file_mention(&node_path::join(&[workspace, file])),
            })
            .collect();
        Ok(FilteredPaneChatSuggestions {
            items,
            is_truncated,
        })
    }

    /// 같은 CLI의 skill·명령 목록을 잠깐 기억해, 메뉴를 열거나 글자를 칠 때마다 설정 폴더를 다시 훑지 않는다.
    /// 훑는 중에 온 요청은 같은 결과를 함께 기다리고, 실패한 조회는 기억하지 않는다.
    fn read_cached_command_catalog(&self, context: &PaneChatCatalogContext) -> CatalogOutcome {
        let key = catalog_key(context);
        let scan = {
            let mut catalogs = self.command_catalogs();
            let cached = catalogs
                .iter()
                .find(|cached| cached.key == key && cached.read_at.elapsed() < COMMAND_CATALOG_TTL);
            if let Some(cached) = cached {
                let scan = Arc::clone(&cached.scan);
                drop(catalogs);
                return scan.wait();
            }
            let scan = Arc::new(CatalogScan::default());
            catalogs.retain(|cached| cached.key != key);
            catalogs.push(CachedCatalog {
                key: key.clone(),
                read_at: Instant::now(),
                scan: Arc::clone(&scan),
            });
            if catalogs.len() > MAX_CACHED_COMMAND_CATALOGS {
                catalogs.remove(0);
            }
            scan
        };
        let outcome = self.scan_command_catalog(context).map(Arc::new);
        scan.finish(outcome.clone());
        if outcome.is_err() {
            self.command_catalogs()
                .retain(|cached| cached.key != key || !Arc::ptr_eq(&cached.scan, &scan));
        }
        outcome
    }

    fn scan_command_catalog(
        &self,
        context: &PaneChatCatalogContext,
    ) -> Result<CommandCatalog, PaneChatCatalogError> {
        let provider = context.provider;
        let roots = self.provider_roots(context)?;
        let catalog_roots = catalog_roots(provider, &project_paths(context), &roots);
        let catalog = match context.ssh_host.as_deref().filter(|host| !host.is_empty()) {
            Some(ssh_host) => {
                read_remote_catalog_files(self.host_files.runner(), ssh_host, &catalog_roots)?
            }
            None => read_local_catalog_files(&catalog_roots),
        };
        let CatalogFiles {
            files,
            is_truncated,
        } = catalog;
        Ok(CommandCatalog {
            skill: list_command_suggestions(provider, &files, PaneChatSuggestionKind::Skill),
            command: list_command_suggestions(provider, &files, PaneChatSuggestionKind::Command),
            is_truncated,
        })
    }

    /// HOME과 CLI 설정 홈. 맥락에 없으면 선택한 CLI 프로세스의 환경에서 읽고, 그것도 안 되면 기본 설치 경로를 쓴다
    fn provider_roots(
        &self,
        context: &PaneChatCatalogContext,
    ) -> Result<ProviderRoots, PaneChatCatalogError> {
        let provider = context.provider;
        let mut home = match &context.home_directory {
            Some(home_directory) => home_directory.clone(),
            None => self
                .host_files
                .home_directory(context.ssh_host.as_deref())?,
        };
        let mut configured_provider = context.provider_home.clone();
        let process_environment = context
            .process_id
            .filter(|process_id| *process_id != 0)
            .and_then(|process_id| self.read_process_environment(context, process_id));
        if let Some(output) = process_environment {
            let variable = match provider {
                PaneChatCatalogProvider::Codex => "CODEX_HOME",
                PaneChatCatalogProvider::Claude => "CLAUDE_CONFIG_DIR",
            };
            if !is_present(context.home_directory.as_deref()) {
                home = read_process_variable(&output, "HOME").unwrap_or(home);
            }
            if !is_present(context.provider_home.as_deref()) {
                configured_provider = read_process_variable(&output, variable);
            }
        }
        let default_directory = match provider {
            PaneChatCatalogProvider::Codex => ".codex",
            PaneChatCatalogProvider::Claude => ".claude",
        };
        let provider =
            configured_provider.unwrap_or_else(|| node_path::join(&[&home, default_directory]));
        Ok(ProviderRoots { home, provider })
    }

    /// 환경 조회 권한이 없으면 `None`이고, 확인 가능한 기본 설치 경로만 제안한다
    fn read_process_environment(
        &self,
        context: &PaneChatCatalogContext,
        process_id: i64,
    ) -> Option<String> {
        let options = ExecGitOptions {
            timeout: Some(CATALOG_TIMEOUT),
            ..ExecGitOptions::default()
        };
        let command = format!("ps eww -p {process_id} -o command=");
        self.host_files
            .runner()
            .exec_git(&command, context.ssh_host.as_deref(), &options)
            .ok()
    }
}
