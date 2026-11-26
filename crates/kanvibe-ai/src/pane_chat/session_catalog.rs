//! pane 채팅 바인딩이 대화 ID를 실제 기록 파일로 확인할 때 쓰는 세션 목록. provider별 기록 리더를 그대로 부른다.

use crate::claude_sessions::read_claude_sessions;
use crate::codex_sessions::{find_codex_main_thread_rollout, read_codex_sessions};
use crate::error::AiResult;
use crate::gemini_sessions::read_gemini_sessions;
use crate::opencode_sessions::read_opencode_sessions;
use crate::pane_chat::process::PaneChatProvider;
use crate::pane_chat::session_bindings::PaneChatSessionCatalog;
use crate::types::{AggregatedAiSession, AiSessionReaderContext};

/// 설치된 기록 리더로 세션을 찾는 목록. TS `sessionReaders`에 해당한다
pub struct RecordedSessionCatalog;

impl PaneChatSessionCatalog for RecordedSessionCatalog {
    fn read_sessions(
        &self,
        provider: PaneChatProvider,
        context: &AiSessionReaderContext,
    ) -> AiResult<Vec<AggregatedAiSession>> {
        let result = match provider {
            PaneChatProvider::Claude => read_claude_sessions(context)?,
            PaneChatProvider::Codex => read_codex_sessions(context)?,
            PaneChatProvider::Gemini => read_gemini_sessions(context)?,
            PaneChatProvider::Opencode => read_opencode_sessions(context)?,
        };
        Ok(result.sessions)
    }

    fn find_codex_main_thread_rollout(
        &self,
        context: &AiSessionReaderContext,
        thread_id: &str,
    ) -> AiResult<Option<String>> {
        find_codex_main_thread_rollout(context, thread_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use base64::Engine;
    use serde_json::json;

    use super::*;
    use crate::host::SessionHost;
    use crate::pane_chat::process::PaneChatProcess;
    use crate::pane_chat::session_bindings::{
        PaneChatBindingContext, PaneChatBindingQuery, PaneChatSessionBinding,
        PaneChatSessionBindings,
    };
    use crate::test_support::{ScriptedRunner, temp_home, write_file};

    const SESSION_ID: &str = "2a366fc3-88b8-4e99-a2f4-6ca4af64d959";

    struct ExactFixture {
        _directory: tempfile::TempDir,
        home: String,
        cwd: String,
    }

    impl ExactFixture {
        /// 메시지 본문에 세션 UUID가 없는 Claude 기록 하나를 심는다
        fn new(project_directory_name: Option<&str>) -> (Self, String) {
            let (directory, home) = temp_home();
            let cwd = format!("{home}/repo");
            let encoded = cwd.replace(['/', '_'], "-");
            let project = format!(
                "{home}/.claude/projects/{}",
                project_directory_name.unwrap_or(&encoded)
            );
            let source_ref = format!("{project}/{SESSION_ID}.jsonl");
            let event = json!({"type": "user", "sessionId": SESSION_ID, "cwd": cwd, "timestamp": "2026-11-16T00:00:00Z", "message": {"role": "user", "content": "A prompt with no identifier"}});
            write_file(
                &source_ref,
                &format!("{}\n", crate::javascript::stringify(&event)),
            );
            (
                Self {
                    _directory: directory,
                    home,
                    cwd,
                },
                source_ref,
            )
        }

        fn reader(&self, output: &str) -> AiSessionReaderContext {
            let output = output.to_string();
            let runner = ScriptedRunner::new(move |_, _| Ok(output.clone()));
            let host = SessionHost::local(&self.home, BTreeMap::new(), runner);
            let mut context = AiSessionReaderContext::new(host, Some(&self.cwd));
            context.repo_path = Some(self.cwd.clone());
            context
        }
    }

    fn exact(source_ref: &str) -> PaneChatSessionBinding {
        PaneChatSessionBinding {
            session_id: SESSION_ID.to_string(),
            source_ref: source_ref.to_string(),
            revision: None,
            is_pending: false,
            is_empty: false,
        }
    }

    fn bindings() -> PaneChatSessionBindings {
        PaneChatSessionBindings::new(Arc::new(RecordedSessionCatalog))
    }

    #[test]
    fn resolves_exact_claude_ids_through_the_real_transcript_reader() {
        let (fixture, source_ref) = ExactFixture::new(None);
        let reported = bindings()
            .find_pane_chat_reported_session_binding(
                &fixture.reader(""),
                PaneChatProvider::Claude,
                SESSION_ID,
            )
            .unwrap();
        assert_eq!(reported, Some(exact(&source_ref)));

        let (fixture, source_ref) = ExactFixture::new(None);
        let started = "Mon Nov 16 08:00:00 2026";
        let reader = fixture.reader(&format!(
            "100 1 {started} claude claude\n300 100 {started} bash bash hook.sh"
        ));
        let registry = bindings();
        let payload =
            crate::javascript::stringify(&json!({"session_id": SESSION_ID, "cwd": fixture.cwd}));
        let metadata = json!({"provider": "claude", "hookPid": 300, "payloadBase64": base64::engine::general_purpose::STANDARD.encode(payload)});
        let context = PaneChatBindingContext {
            task_id: fixture.home.clone(),
            reader,
        };
        registry.record_pane_chat_session_binding(&context, &metadata);
        let query = PaneChatBindingQuery {
            task_id: fixture.home.clone(),
            ssh_host: None,
            provider: PaneChatProvider::Claude,
            provider_pid: 100,
            process_start_time: started.to_string(),
        };
        let hooked = registry
            .find_pane_chat_session_binding(&query)
            .unwrap()
            .unwrap();
        assert_eq!(
            (hooked.session_id.as_str(), hooked.source_ref.as_str()),
            (SESSION_ID, source_ref.as_str())
        );

        let (fixture, source_ref) = ExactFixture::new(Some("migrated-project-directory"));
        write_file(
            &format!("{}/.claude/sessions/100.json", fixture.home),
            &crate::javascript::stringify(
                &json!({"pid": 100, "sessionId": SESSION_ID, "cwd": fixture.cwd, "kind": "interactive", "procStart": "12345", "pidDomain": "linux:machine:pid:[1]"}),
            ),
        );
        let stat = format!(
            "100 (claude) S {} 12345 0\nlinux:machine:pid:[1]\n",
            vec!["0"; 18].join(" ")
        );
        let process = PaneChatProcess {
            pid: 100,
            process_start_time: "current".to_string(),
            provider: PaneChatProvider::Claude,
            codex_server_socket: None,
        };
        let found =
            bindings().find_pane_chat_process_session_binding(&fixture.reader(&stat), &process);
        assert_eq!(found, Some(exact(&source_ref)));
    }
}
