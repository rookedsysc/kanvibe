//! `kanban_tasks` 테이블의 행과 그 상태 어휘.

use serde::{Deserialize, Serialize};

/// 보드 컬럼이자 hook이 바꾸는 작업 상태. DB와 모바일 JSON에는 소문자 이름 그대로 저장된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Todo,
    Progress,
    Pending,
    Review,
    Done,
}

/// 보드 컬럼과 같은 상태 진행 순서. vim 이동 명령과 커맨드 팔레트가 이 순서를 공유한다.
pub const TASK_STATUS_ORDER: [TaskStatus; 5] = [
    TaskStatus::Todo,
    TaskStatus::Progress,
    TaskStatus::Pending,
    TaskStatus::Review,
    TaskStatus::Done,
];

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Progress => "progress",
            Self::Pending => "pending",
            Self::Review => "review",
            Self::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        TASK_STATUS_ORDER
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

/// 작업 터미널을 담는 세션 종류. `Terminal`은 멀티플렉서 없이 KanVibe가 PTY와 탭을 직접 소유한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionType {
    Tmux,
    Zellij,
    Herdr,
    Terminal,
}

impl SessionType {
    pub const ALL: [SessionType; 4] = [Self::Tmux, Self::Zellij, Self::Herdr, Self::Terminal];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tmux => "tmux",
            Self::Zellij => "zellij",
            Self::Herdr => "herdr",
            Self::Terminal => "terminal",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|session_type| session_type.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
}

impl TaskPriority {
    pub const ALL: [TaskPriority; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|priority| priority.as_str() == value)
    }
}

/// 보드의 작업 하나. 시각 값은 SQLite에 저장된 문자열을 바꾸지 않고 그대로 들고 다닌다.
/// TS 앱이 쓴 `2026-01-02 00:00:00`과 ISO 형식이 한 DB에 섞여 있어, 읽을 때 변환하면 다시 쓸 때 원본이 바뀐다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KanbanTask {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub branch_name: Option<String>,
    pub worktree_path: Option<String>,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
    pub agent_type: Option<String>,
    pub project_id: Option<String>,
    pub base_branch: Option<String>,
    pub pr_url: Option<String>,
    pub priority: Option<TaskPriority>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_names_match_the_stored_lowercase_values() {
        let names: Vec<&str> = TASK_STATUS_ORDER
            .iter()
            .map(|status| status.as_str())
            .collect();

        assert_eq!(names, ["todo", "progress", "pending", "review", "done"]);
        assert_eq!(
            serde_json::to_string(&TaskStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(TaskStatus::parse("review"), Some(TaskStatus::Review));
        assert_eq!(TaskStatus::parse("REVIEW"), None);
    }

    #[test]
    fn session_type_and_priority_round_trip_through_their_stored_names() {
        for session_type in SessionType::ALL {
            assert_eq!(
                SessionType::parse(session_type.as_str()),
                Some(session_type)
            );
        }
        for priority in TaskPriority::ALL {
            let json = serde_json::to_string(&priority).unwrap();
            assert_eq!(json, format!("\"{}\"", priority.as_str()));
        }
        assert_eq!(SessionType::parse("screen"), None);
    }
}
