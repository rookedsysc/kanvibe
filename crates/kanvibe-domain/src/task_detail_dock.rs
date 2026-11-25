//! 작업 상세 도크 항목의 순서. `Mod+{n}` 번호와 단축키 설정 화면의 라벨이 모두 이 순서에서 나온다.
//! PR 항목은 링크가 없어도 자리를 지킨다. 자리가 사라지면 뒤 항목 번호가 통째로 밀려 근육기억이 깨진다.
//! 보드 복귀 버튼과 사용량 버튼은 이 목록 밖이라 번호를 받지 않는다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskDetailDockItemId {
    Overview,
    Status,
    Chat,
    PullRequest,
    LiveSessions,
    Vscode,
}

pub const TASK_DETAIL_DOCK_ITEM_IDS: [TaskDetailDockItemId; 6] = [
    TaskDetailDockItemId::Overview,
    TaskDetailDockItemId::Status,
    TaskDetailDockItemId::Chat,
    TaskDetailDockItemId::PullRequest,
    TaskDetailDockItemId::LiveSessions,
    TaskDetailDockItemId::Vscode,
];

/// 도크 툴팁과 단축키 설정 화면이 함께 쓰는 항목 이름. `translate`는 `taskDetail` 네임스페이스의 번역기다.
pub fn resolve_task_detail_dock_label(
    translate: impl Fn(&str) -> String,
    item_id: TaskDetailDockItemId,
) -> String {
    match item_id {
        TaskDetailDockItemId::Overview => translate("info"),
        TaskDetailDockItemId::Status => {
            format!("{} · {}", translate("actions"), translate("hooksStatus"))
        }
        TaskDetailDockItemId::Chat => translate("aiSessions.inlineChat"),
        TaskDetailDockItemId::PullRequest => "PR".to_string(),
        TaskDetailDockItemId::LiveSessions => translate("liveSessions.dock"),
        TaskDetailDockItemId::Vscode => translate("openInVsCode"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Locale, Translator};

    #[test]
    fn dock_labels_come_from_the_task_detail_messages() {
        let translator = Translator::new(Locale::En, "taskDetail");
        let labels: Vec<String> = TASK_DETAIL_DOCK_ITEM_IDS
            .into_iter()
            .map(|item_id| resolve_task_detail_dock_label(|key| translator.translate(key), item_id))
            .collect();

        assert_eq!(labels[3], "PR");
        assert!(
            labels.iter().all(|label| !label.starts_with("taskDetail.")),
            "{labels:?}"
        );
        assert_eq!(
            serde_json::to_string(&TaskDetailDockItemId::PullRequest).unwrap(),
            "\"pull-request\""
        );
    }
}
