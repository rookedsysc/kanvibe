use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use kanvibe_domain::pane_chat::{FilteredPaneChatSuggestions, PaneChatSuggestion};
use kanvibe_process::exec::local::exec_local;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::ShellEnvironment;

use super::{
    PaneChatCatalogContext, PaneChatCatalogProvider, PaneChatComposerCatalog,
    PaneChatSuggestionKind,
};
use crate::test_support::ScriptedShellRunner;

const DEFAULT_LIMIT: usize = 20;

/// 로컬 `git`만 실제로 실행하는 실행기. 파일 제안이 git ignore 규칙을 따르는지 git 자체로 확인한다
struct LocalGitRunner {
    environment: ShellEnvironment,
}

impl ShellCommandRunner for LocalGitRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        assert_eq!(ssh_host, None, "원격 명령을 보내면 안 된다");
        exec_local(command, &self.environment, options)
    }
}

fn catalog_with(runner: Arc<dyn ShellCommandRunner>) -> PaneChatComposerCatalog {
    PaneChatComposerCatalog::new(Arc::new(HostFileAccess::new(runner, "/unused-home")))
}

fn codex_context(cwd: &Path, home: &Path) -> PaneChatCatalogContext {
    PaneChatCatalogContext {
        provider: PaneChatCatalogProvider::Codex,
        cwd: cwd.to_str().unwrap().to_string(),
        ssh_host: None,
        home_directory: Some(home.to_str().unwrap().to_string()),
        provider_home: None,
        process_id: None,
        workspace_path: None,
    }
}

fn remote_context(ssh_host: &str) -> PaneChatCatalogContext {
    PaneChatCatalogContext {
        provider: PaneChatCatalogProvider::Codex,
        cwd: "/remote/repo".to_string(),
        ssh_host: Some(ssh_host.to_string()),
        home_directory: Some("/remote/home".to_string()),
        provider_home: Some("/remote/home/.codex".to_string()),
        process_id: None,
        workspace_path: None,
    }
}

fn write_skill(root: &Path, name: &str, body: &str) {
    let target = root.join(name);
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("SKILL.md"), body).unwrap();
}

fn write_numbered_skills(cwd: &Path, count: usize) {
    for index in 0..count {
        let body = format!("---\nname: skill-{index}\ndescription: Result\n---\n");
        write_skill(
            &cwd.join(".agents/skills"),
            &format!("skill-{index}"),
            &body,
        );
    }
}

fn labels(result: &FilteredPaneChatSuggestions) -> Vec<&str> {
    result
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect()
}

fn find_label<'a>(result: &'a FilteredPaneChatSuggestions, label: &str) -> &'a PaneChatSuggestion {
    result
        .items
        .iter()
        .find(|item| item.label == label)
        .unwrap()
}

#[test]
fn prefers_project_skills_follows_symlinks_and_skips_malformed_skill() {
    let directory = tempfile::tempdir().unwrap();
    let (home, cwd) = (directory.path().join("home"), directory.path().join("repo"));
    write_skill(
        &home.join(".agents/skills"),
        "review",
        "---\nname: review\ndescription: Home review\n---\n",
    );
    write_skill(
        &cwd.join(".agents/skills"),
        "review",
        "---\nname: review\ndescription: Project review\n---\n",
    );
    write_skill(
        &cwd.join(".agents/skills"),
        "bad",
        "---\nname: [invalid\n---\n",
    );
    write_skill(
        &directory.path().join("external"),
        "linked",
        "---\nname: linked\ndescription: Linked skill\n---\n",
    );
    symlink(
        directory.path().join("external/linked"),
        cwd.join(".agents/skills/linked"),
    )
    .unwrap();
    let catalog = catalog_with(ScriptedShellRunner::new());

    let result = catalog
        .read_pane_chat_suggestions(
            &codex_context(&cwd, &home),
            PaneChatSuggestionKind::Skill,
            "",
            DEFAULT_LIMIT,
        )
        .unwrap();

    assert_eq!(labels(&result), vec!["linked", "review"]);
    let review = find_label(&result, "review");
    assert_eq!(review.description, "Project review");
    assert_eq!(review.insert_text, "$review");
}

#[test]
fn uses_claude_slash_commands_and_hides_non_user_invocable_skills() {
    let directory = tempfile::tempdir().unwrap();
    let (home, cwd) = (directory.path().join("home"), directory.path().join("repo"));
    write_skill(
        &cwd.join(".claude/skills"),
        "visible",
        "---\nname: visible\ndescription: Visible\n---\n",
    );
    write_skill(
        &cwd.join(".claude/skills"),
        "hidden",
        "---\nname: hidden\nuser-invocable: false\n---\n",
    );
    fs::create_dir_all(cwd.join(".claude/commands")).unwrap();
    fs::write(
        cwd.join(".claude/commands/check.md"),
        "---\ndescription: Check files\n---\nRun checks",
    )
    .unwrap();
    let catalog = catalog_with(ScriptedShellRunner::new());
    let context = PaneChatCatalogContext {
        provider: PaneChatCatalogProvider::Claude,
        ..codex_context(&cwd, &home)
    };

    let skills = catalog
        .read_pane_chat_suggestions(&context, PaneChatSuggestionKind::Skill, "", DEFAULT_LIMIT)
        .unwrap();
    let commands = catalog
        .read_pane_chat_suggestions(
            &context,
            PaneChatSuggestionKind::Command,
            "check",
            DEFAULT_LIMIT,
        )
        .unwrap();

    let skill_inserts: Vec<&str> = skills
        .items
        .iter()
        .map(|item| item.insert_text.as_str())
        .collect();
    assert_eq!(skill_inserts, vec!["/visible"]);
    assert_eq!(labels(&commands), vec!["check"]);
    assert_eq!(commands.items[0].insert_text, "/check");
}

fn init_git_repository(cwd: &Path, home: &Path) {
    let status = Command::new("git")
        .args(["init", "-q"])
        .arg(cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn suggests_repo_files_with_safe_mentions_respecting_git_ignores() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("repo");
    fs::create_dir_all(&cwd).unwrap();
    init_git_repository(&cwd, directory.path());
    fs::write(cwd.join("design notes.md"), "notes").unwrap();
    fs::write(cwd.join(".gitignore"), "secret.txt\n").unwrap();
    fs::write(cwd.join("secret.txt"), "ignored").unwrap();
    fs::write(cwd.join(" design leading.md"), "leading space").unwrap();
    let environment: ShellEnvironment = [
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        (
            "HOME".to_string(),
            directory.path().to_str().unwrap().to_string(),
        ),
    ]
    .into();
    let catalog = catalog_with(Arc::new(LocalGitRunner { environment }));
    let context = codex_context(&cwd, directory.path());
    let read = |query: &str| {
        catalog
            .read_pane_chat_suggestions(
                &context,
                PaneChatSuggestionKind::File,
                query,
                DEFAULT_LIMIT,
            )
            .unwrap()
    };
    let cwd_text = cwd.to_str().unwrap();

    let notes = read("design notes");
    assert_eq!(labels(&notes), vec!["design notes.md"]);
    assert_eq!(
        notes.items[0].insert_text,
        format!("@\"{cwd_text}/design notes.md\"")
    );
    assert!(read("secret").items.is_empty());
    let leading = read("leading");
    assert_eq!(leading.items[0].label, " design leading.md");
    assert_eq!(
        leading.items[0].insert_text,
        format!("@\"{cwd_text}/ design leading.md\"")
    );
}

#[test]
fn uses_selected_cli_process_custom_provider_home() {
    let directory = tempfile::tempdir().unwrap();
    let (home, cwd) = (directory.path().join("home"), directory.path().join("repo"));
    let custom = directory.path().join("custom provider home");
    write_skill(
        &custom.join("skills"),
        "custom",
        "---\nname: custom\ndescription: Custom home\n---\n",
    );
    let runner = ScriptedShellRunner::new();
    let process_environment = format!(
        "node -e setTimeout(() => {{}}, 10000) HOME={} CODEX_HOME={} LANG=C.UTF-8",
        home.to_str().unwrap(),
        custom.to_str().unwrap()
    );
    runner.respond_when_contains("ps eww -p 4242 -o command=", &process_environment);
    let catalog = catalog_with(runner.clone());
    let context = PaneChatCatalogContext {
        process_id: Some(4242),
        ..codex_context(&cwd, &home)
    };

    let result = catalog
        .read_pane_chat_suggestions(
            &context,
            PaneChatSuggestionKind::Skill,
            "custom",
            DEFAULT_LIMIT,
        )
        .unwrap();

    assert_eq!(labels(&result), vec!["custom"]);
    assert_eq!(result.items[0].insert_text, "$custom");
    assert_eq!(runner.commands(), vec!["ps eww -p 4242 -o command="]);
}

#[test]
fn reads_remote_skill_bytes_only_through_selected_ssh_host() {
    let skill_path = "/remote/repo/.agents/skills/remote/SKILL.md";
    let content = STANDARD.encode("---\nname: remote\ndescription: Remote only\n---\n");
    let runner = ScriptedShellRunner::new();
    runner.respond_when_contains("'/remote/repo/.agents/skills'", &format!("{skill_path}\0"));
    runner.respond(move |command, _| {
        command
            .starts_with("for __kanvibe_skill")
            .then(|| Ok(format!("{skill_path}\0{content}\0")))
    });
    let catalog = catalog_with(runner.clone());

    let result = catalog
        .read_pane_chat_suggestions(
            &remote_context("fixture-host"),
            PaneChatSuggestionKind::Skill,
            "remote",
            DEFAULT_LIMIT,
        )
        .unwrap();

    assert_eq!(labels(&result), vec!["remote"]);
    assert_eq!(result.items[0].insert_text, "$remote");
    assert!(
        runner
            .calls()
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("fixture-host"))
    );
}

#[test]
fn bounds_visible_suggestions_and_announces_more_matches() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("repo");
    write_numbered_skills(&cwd, 21);
    let catalog = catalog_with(ScriptedShellRunner::new());

    let result = catalog
        .read_pane_chat_suggestions(
            &codex_context(&cwd, directory.path()),
            PaneChatSuggestionKind::Skill,
            "skill-",
            DEFAULT_LIMIT,
        )
        .unwrap();

    assert_eq!(result.items.len(), 20);
    assert!(result.is_truncated);
}

#[test]
fn returns_whole_catalog_when_caller_raises_limit() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("repo");
    write_numbered_skills(&cwd, 21);
    let catalog = catalog_with(ScriptedShellRunner::new());

    let result = catalog
        .read_pane_chat_suggestions(
            &codex_context(&cwd, directory.path()),
            PaneChatSuggestionKind::Skill,
            "",
            512,
        )
        .unwrap();

    assert_eq!(result.items.len(), 21);
    assert!(!result.is_truncated);
}

/// 캐시에 있는 목록이 `age`만큼 전에 훑은 것처럼 만든다(vitest 가짜 시계 대신)
fn age_cached_catalogs(catalog: &PaneChatComposerCatalog, age: Duration) {
    for cached in catalog.command_catalogs().iter_mut() {
        cached.read_at = Instant::now() - age;
    }
}

#[test]
fn reuses_scanned_catalog_for_ten_seconds_then_picks_up_new_skill() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("repo");
    let context = codex_context(&cwd, directory.path());
    let catalog = catalog_with(ScriptedShellRunner::new());
    let read_labels = || {
        let result = catalog
            .read_pane_chat_suggestions(&context, PaneChatSuggestionKind::Skill, "", DEFAULT_LIMIT)
            .unwrap();
        result
            .items
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
    };

    write_skill(
        &cwd.join(".agents/skills"),
        "first",
        "---\nname: first\ndescription: First\n---\n",
    );
    assert_eq!(read_labels(), vec!["first"]);
    write_skill(
        &cwd.join(".agents/skills"),
        "second",
        "---\nname: second\ndescription: Second\n---\n",
    );
    age_cached_catalogs(&catalog, Duration::from_millis(9_999));
    assert_eq!(read_labels(), vec!["first"]);
    age_cached_catalogs(&catalog, Duration::from_millis(10_000));
    assert_eq!(read_labels(), vec!["first", "second"]);
}

#[test]
fn shares_one_scan_between_concurrent_requests() {
    let runner = ScriptedShellRunner::new();
    let catalog = catalog_with(runner.clone());
    let context = remote_context("shared-scan-host");

    thread::scope(|scope| {
        for kind in [
            PaneChatSuggestionKind::Skill,
            PaneChatSuggestionKind::Command,
        ] {
            let (catalog, context) = (&catalog, &context);
            scope.spawn(move || {
                catalog
                    .read_pane_chat_suggestions(context, kind, "", DEFAULT_LIMIT)
                    .unwrap()
            });
        }
    });

    let scanned_roots = runner
        .commands()
        .into_iter()
        .filter(|command| command.contains("'/remote/home/.codex/skills'"))
        .count();
    assert_eq!(scanned_roots, 1);
}

#[test]
fn does_not_remember_failed_scan() {
    let runner = ScriptedShellRunner::new();
    let has_failed = AtomicBool::new(false);
    runner.respond(move |_, _| {
        (!has_failed.swap(true, Ordering::SeqCst))
            .then(|| Err(ExecGitError::new("ssh unavailable")))
    });
    let catalog = catalog_with(runner);
    let context = remote_context("failing-scan-host");

    let error = catalog
        .read_pane_chat_suggestions(&context, PaneChatSuggestionKind::Skill, "", DEFAULT_LIMIT)
        .unwrap_err();
    let retried = catalog
        .read_pane_chat_suggestions(&context, PaneChatSuggestionKind::Skill, "", DEFAULT_LIMIT)
        .unwrap();

    assert_eq!(error.to_string(), "ssh unavailable");
    assert_eq!(
        retried,
        FilteredPaneChatSuggestions {
            items: Vec::new(),
            is_truncated: false
        }
    );
}

#[test]
fn reads_last_absolute_process_variable_and_rejects_relative_values() {
    let output = "cli HOME=/first CODEX_HOME=/a b/c PATH=/bin CODEX_HOME=relative";

    assert_eq!(
        super::read_process_variable(output, "HOME"),
        Some("/first".to_string())
    );
    assert_eq!(super::read_process_variable(output, "CODEX_HOME"), None);
    assert_eq!(
        super::read_process_variable("CODEX_HOME=/a b/c LANG=C", "CODEX_HOME"),
        Some("/a b/c".to_string())
    );
}

#[test]
fn reads_folded_description_and_quoted_names() {
    let content = "---\nname: \"quoted\"\ndescription: >\n  first line\n  second   line\n---\nbody";

    let metadata = super::skill_metadata::read_skill_metadata(content, "fallback").unwrap();

    assert_eq!(metadata.name, "quoted");
    assert_eq!(metadata.description, "first line second line");
}
