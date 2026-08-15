import { filterPanesByWorktree } from "@/desktop/shared/liveAiSessions";
import { listRunningAgentPanes } from "@/lib/aiSessions/runningAgentPanes";
import { readClaudeLiveSessions } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexLiveSessions } from "@/lib/aiSessions/readCodexSessions";
import { readGeminiLiveSessions } from "@/lib/aiSessions/readGeminiSessions";
import { readOpenCodeLiveSessions } from "@/lib/aiSessions/readOpenCodeSessions";
import type {
  AiSessionProvider,
  AiSessionReaderContext,
  LiveAiSession,
  LiveAiSessionWindows,
  LiveProviderSnapshot,
  RunningAgentPane,
} from "@/lib/aiSessions/types";

/** 이 시간 안에 기록이 갱신됐으면 세션이 일하고 있다고 본다 */
const RUNNING_WINDOW_MS = 90_000;

/** 이 시간 안에 움직인 세션까지만 유휴 상태로 보여준다. 더 오래된 세션은 목록에서 뺀다 */
const RECENT_WINDOW_MS = 6 * 60 * 60 * 1000;

export const LIVE_SESSION_WINDOWS: LiveAiSessionWindows = {
  runningWindowMs: RUNNING_WINDOW_MS,
  recentWindowMs: RECENT_WINDOW_MS,
};

const LIVE_SESSION_READERS: Record<
  AiSessionProvider,
  (context: AiSessionReaderContext, windows: LiveAiSessionWindows) => Promise<LiveProviderSnapshot[]>
> = {
  claude: readClaudeLiveSessions,
  codex: readCodexLiveSessions,
  opencode: readOpenCodeLiveSessions,
  gemini: readGeminiLiveSessions,
};

const PROVIDERS = Object.keys(LIVE_SESSION_READERS) as AiSessionProvider[];

/**
 * 한 worktree에서 지금 돌고 있는 AI 세션과 그 세션이 띄운 서브태스크를 모은다.
 *
 * 판정은 두 근거를 겹쳐 쓴다. tmux pane에 에이전트 프로세스가 붙어 있으면 입력을 기다리며 놀고 있어도
 * 실행중이고, pane이 보이지 않아도 세션 기록이 방금 갱신됐으면 실행중이다.
 * 앞의 근거는 tmux 밖(zellij, KanVibe terminal 세션, 외부 터미널)을 못 보고 뒤의 근거는 대기 상태를
 * 못 보기 때문에, 둘 중 하나라도 성립하면 실행중으로 판단한다.
 */
export async function readLiveAiSessions(
  context: AiSessionReaderContext,
  runningPanes?: RunningAgentPane[],
): Promise<LiveAiSession[]> {
  const panes = filterPanesByWorktree(
    runningPanes ?? await listRunningAgentPanes(context.sshHost),
    context.worktreePath,
  );

  const snapshotsByProvider = await Promise.all(PROVIDERS.map(async (provider) => ({
    provider,
    snapshots: await readSnapshotsSafely(provider, context),
  })));

  const runningSince = Date.now() - RUNNING_WINDOW_MS;

  return snapshotsByProvider.flatMap(({ provider, snapshots }) => toProviderLiveSessions(
    provider,
    snapshots,
    panes.filter((pane) => pane.provider === provider),
    runningSince,
  ));
}

/**
 * 한 provider의 세션 기록과 pane을 한 줄씩 맞춘다.
 *
 * pane은 어떤 에이전트가 어느 worktree에서 도는지만 알려줄 뿐 세션 id를 담지 않아, 세션과 pane을
 * 정확히 이어 붙일 근거가 없다. 그래서 양쪽을 최근 순으로 늘어놓고 자리끼리 맞춘다.
 * 한쪽이 더 많으면 남는 자리는 비워 두는데, 기록을 못 읽은 pane도 개수에서 빠지면 안 되고
 * pane 밖(zellij, 외부 터미널)에서 도는 세션도 목록에서 사라지면 안 되기 때문이다.
 */
function toProviderLiveSessions(
  provider: AiSessionProvider,
  snapshots: LiveProviderSnapshot[],
  providerPanes: RunningAgentPane[],
  runningSince: number,
): LiveAiSession[] {
  const sessionCount = Math.max(snapshots.length, providerPanes.length);

  return Array.from({ length: sessionCount }, (_unusedValue, index) => toLiveSession(
    provider,
    snapshots[index] ?? null,
    providerPanes[index] ?? null,
    runningSince,
  ));
}

function toLiveSession(
  provider: AiSessionProvider,
  snapshot: LiveProviderSnapshot | null,
  pane: RunningAgentPane | null,
  runningSince: number,
): LiveAiSession {
  const lastActiveAt = snapshot?.lastActiveAt ?? null;
  const hasRecentActivity = lastActiveAt !== null && Date.parse(lastActiveAt) >= runningSince;

  return {
    provider,
    sessionId: snapshot?.sessionId ?? null,
    currentTask: snapshot?.currentTask ?? null,
    state: pane || hasRecentActivity ? "running" : "idle",
    lastActiveAt,
    runningSubtasks: snapshot?.runningSubtasks ?? [],
    terminalWindow: pane
      ? { sessionName: pane.sessionName, windowId: pane.windowId, windowName: pane.windowName }
      : null,
  };
}

/**
 * 리더 하나가 실패해도 나머지 provider는 보여준다.
 * 폴링으로 계속 다시 부르는 조회라, 원격 연결이 끊긴 순간을 오류로 띄우면 화면이 오류로 덮인다.
 */
async function readSnapshotsSafely(
  provider: AiSessionProvider,
  context: AiSessionReaderContext,
): Promise<LiveProviderSnapshot[]> {
  try {
    return await LIVE_SESSION_READERS[provider](context, LIVE_SESSION_WINDOWS);
  } catch {
    return [];
  }
}
