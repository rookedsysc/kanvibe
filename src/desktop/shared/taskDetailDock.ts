/**
 * 작업 상세 도크에 놓인 항목의 순서. `Mod+{n}` 단축키 번호와 단축키 설정 화면의 라벨이 모두 여기서 나온다.
 * 항목을 넣거나 빼면 뒤 항목의 단축키 번호가 함께 움직인다.
 * PR 항목은 링크가 아직 없어도 자리를 지킨다. 자리가 사라지면 뒤 항목의 번호가 통째로 밀려 근육기억이 깨진다.
 * 보드 복귀 버튼과 사용량 버튼은 이 목록 밖이라 번호를 받지 않는다.
 */
export const TASK_DETAIL_DOCK_ITEM_IDS = [
  "overview",
  "status",
  "chat",
  "pull-request",
  "live-sessions",
  "vscode",
] as const;

export type TaskDetailDockItemId = typeof TASK_DETAIL_DOCK_ITEM_IDS[number];

/** `taskDetail` 메시지 번역기. 도크와 설정 화면이 라벨 정의를 공유하기 위한 최소 계약 */
export type TaskDetailMessageTranslator = (key: string) => string;

/**
 * 도크 항목의 사람이 읽는 이름.
 * 도크 툴팁과 단축키 설정 화면이 이 한 함수를 함께 써야, 설정 화면이 "도크 4번 항목"처럼
 * 정체를 감추거나 도크의 이름이 바뀐 뒤 둘이 갈라지는 일이 생기지 않는다.
 */
export function resolveTaskDetailDockLabel(
  t: TaskDetailMessageTranslator,
  itemId: TaskDetailDockItemId,
): string {
  switch (itemId) {
    case "overview":
      return t("info");
    case "status":
      return `${t("actions")} · ${t("hooksStatus")}`;
    case "chat":
      return t("aiSessions.inlineChat");
    case "pull-request":
      return "PR";
    case "live-sessions":
      return t("liveSessions.dock");
    case "vscode":
      return t("openInVsCode");
  }
}
