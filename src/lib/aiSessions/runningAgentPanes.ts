import { execGit } from "@/lib/gitOperations";
import { buildTmuxListPanesCommand, parseTmuxPaneList, type TmuxPaneSnapshot } from "@/lib/terminalTabs";
import type { AiSessionProvider, RunningAgentPane } from "@/lib/aiSessions/types";

/**
 * tmux pane이 실행중이라고 알려주는 에이전트 명령어.
 * pane_current_command는 실행 파일 이름이므로 CLI 이름과 그대로 대응한다.
 */
const PROVIDER_BY_PANE_COMMAND: Record<string, AiSessionProvider> = {
  claude: "claude",
  codex: "codex",
  opencode: "opencode",
  gemini: "gemini",
};

/**
 * tmux 서버 전체를 한 번 훑어 실행중인 에이전트 pane을 모은다.
 *
 * 프로세스 목록을 직접 뒤지지 않는 이유는 두 가지다. pane 조회 한 번으로 실행 여부와 작업 경로를
 * 같이 얻을 수 있고, 세션을 클릭했을 때 전환할 window id까지 같은 줄에 들어 있다.
 * 대신 tmux 밖에서 도는 에이전트(zellij, KanVibe terminal 세션, 별도 터미널 앱)는 보이지 않으므로,
 * 호출자는 세션 기록의 최근 활동을 폴백으로 함께 봐야 한다.
 */
export async function listRunningAgentPanes(sshHost?: string | null): Promise<RunningAgentPane[]> {
  const panes = await listTmuxPanes(sshHost);

  return panes
    .map(toRunningAgentPane)
    .filter((pane): pane is RunningAgentPane => pane !== null);
}

/** tmux 세션이 하나도 없으면 tmux는 오류로 끝난다. 실행중인 에이전트가 없는 정상 상태이므로 빈 목록으로 다룬다 */
async function listTmuxPanes(sshHost?: string | null): Promise<TmuxPaneSnapshot[]> {
  try {
    return parseTmuxPaneList(await execGit(buildTmuxListPanesCommand(), sshHost));
  } catch {
    return [];
  }
}

function toRunningAgentPane(pane: TmuxPaneSnapshot): RunningAgentPane | null {
  const provider = PROVIDER_BY_PANE_COMMAND[pane.command];
  if (!provider) {
    return null;
  }

  return {
    provider,
    worktreePath: pane.currentPath,
    sessionName: pane.sessionName,
    windowId: pane.windowId,
    windowName: pane.windowName,
  };
}
