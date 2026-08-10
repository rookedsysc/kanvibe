/**
 * @vitest-environment node
 */
import { mkdir, mkdtemp, rm, utimes, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { filterPanesByWorktree } from "@/desktop/shared/liveAiSessions";
import type { RunningAgentPane } from "@/lib/aiSessions/types";

let tempHome: string;
let worktreePath: string;

const RUNNING_WINDOW_MS = 90_000;

async function writeJsonLines(filePath: string, values: unknown[], ageMs = 0) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${values.map((value) => JSON.stringify(value)).join("\n")}\n`, "utf-8");

  const modifiedAt = new Date(Date.now() - ageMs);
  await utimes(filePath, modifiedAt, modifiedAt);
}

function claudeProjectDirectoryName(targetPath: string) {
  return path.resolve(targetPath).replaceAll(path.sep, "-").replaceAll("_", "-");
}

function claudeSessionFile(sessionId: string) {
  return path.join(tempHome, ".claude", "projects", claudeProjectDirectoryName(worktreePath), `${sessionId}.jsonl`);
}

function claudeSubagentFile(sessionId: string, agentId: string) {
  return path.join(
    tempHome,
    ".claude",
    "projects",
    claudeProjectDirectoryName(worktreePath),
    sessionId,
    "subagents",
    `agent-${agentId}.jsonl`,
  );
}

/** tmux를 실제로 띄우지 않고 pane 목록만 흉내 낸다 */
function mockTmuxPanes(panes: string[]) {
  vi.doMock("@/lib/gitOperations", () => ({
    execGit: vi.fn(async (command: string) => {
      if (command.startsWith("tmux list-panes")) {
        return panes.join("\n");
      }

      throw new Error(`unexpected command: ${command}`);
    }),
    isSSHTransportError: () => false,
  }));
}

async function readLiveSessions() {
  const { readLiveAiSessions } = await import("@/lib/aiSessions/liveAiSessions");
  return readLiveAiSessions({ worktreePath, repoPath: worktreePath });
}

/**
 * OpenCode 리더가 better-sqlite3 네이티브 모듈을 처음 불러올 때 몇 초가 걸린다.
 * 그 비용은 파일에서 먼저 도는 테스트 하나가 통째로 떠안으므로 기본 5초로는 부족하다.
 */
const NATIVE_SQLITE_LOAD_TIMEOUT_MS = 30_000;

describe("실행중 AI 세션 감지", { timeout: NATIVE_SQLITE_LOAD_TIMEOUT_MS }, () => {
  beforeEach(async () => {
    tempHome = await mkdtemp(path.join(tmpdir(), "kanvibe-live-sessions-"));
    worktreePath = path.join(tempHome, "repo__worktrees", "task");
    await mkdir(worktreePath, { recursive: true });
    vi.stubEnv("HOME", tempHome);
  });

  afterEach(async () => {
    vi.doUnmock("@/lib/gitOperations");
    vi.resetModules();
    vi.unstubAllEnvs();
    await rm(tempHome, { recursive: true, force: true });
  });

  it("세션 기록이 방금 갱신됐으면 tmux pane이 없어도 실행중으로 본다", async () => {
    mockTmuxPanes([]);
    await writeJsonLines(claudeSessionFile("session-a"), [{ type: "user" }], 1_000);

    const sessions = await readLiveSessions();
    const claude = sessions.find((session) => session.provider === "claude");

    expect(claude?.state).toBe("running");
    expect(claude?.sessionId).toBe("session-a");
    expect(claude?.terminalWindow).toBeNull();
  });

  it("한동안 갱신이 없으면 유휴로 본다", async () => {
    mockTmuxPanes([]);
    await writeJsonLines(claudeSessionFile("session-a"), [{ type: "user" }], RUNNING_WINDOW_MS + 60_000);

    const sessions = await readLiveSessions();

    expect(sessions.find((session) => session.provider === "claude")?.state).toBe("idle");
  });

  it("입력을 기다리며 놀고 있어도 tmux pane이 붙어 있으면 실행중으로 본다", async () => {
    mockTmuxPanes([`kanvibe-task\t@7\t0\tclaude\t${worktreePath}\tclaude`]);
    await writeJsonLines(claudeSessionFile("session-a"), [{ type: "user" }], RUNNING_WINDOW_MS + 60_000);

    const sessions = await readLiveSessions();
    const claude = sessions.find((session) => session.provider === "claude");

    expect(claude?.state).toBe("running");
    expect(claude?.terminalWindow).toEqual({
      sessionName: "kanvibe-task",
      windowId: "@7",
      windowName: "claude",
    });
  });

  it("최근에 움직인 서브에이전트만 실행중 서브태스크로 센다", async () => {
    mockTmuxPanes([]);
    await writeJsonLines(claudeSessionFile("session-a"), [{ type: "user" }], 1_000);
    await writeJsonLines(
      claudeSubagentFile("session-a", "running-agent"),
      [{ agentId: "running-agent", message: { role: "user", content: [{ type: "text", text: "코드베이스를 조사한다" }] } }],
      5_000,
    );
    await writeJsonLines(
      claudeSubagentFile("session-a", "finished-agent"),
      [{ agentId: "finished-agent", message: { role: "user", content: [{ type: "text", text: "이미 끝난 작업" }] } }],
      RUNNING_WINDOW_MS + 60_000,
    );

    const sessions = await readLiveSessions();
    const claude = sessions.find((session) => session.provider === "claude");

    expect(claude?.runningSubtasks).toEqual([
      expect.objectContaining({ id: "running-agent", name: "코드베이스를 조사한다" }),
    ]);
  });

  it("Codex 서브에이전트는 부모 스레드가 같은 rollout만 센다", async () => {
    mockTmuxPanes([]);
    const codexSessions = path.join(tempHome, ".codex", "sessions", "2026", "08", "10");
    const sessionMeta = (payload: Record<string, unknown>) => ({ type: "session_meta", payload });

    await writeJsonLines(
      path.join(codexSessions, "rollout-parent.jsonl"),
      [sessionMeta({ id: "parent-thread", cwd: worktreePath })],
      1_000,
    );
    await writeJsonLines(
      path.join(codexSessions, "rollout-child.jsonl"),
      [sessionMeta({ id: "child-thread", cwd: worktreePath, parent_thread_id: "parent-thread", agent_nickname: "Fermat" })],
      2_000,
    );
    await writeJsonLines(
      path.join(codexSessions, "rollout-other.jsonl"),
      [sessionMeta({ id: "other-child", cwd: worktreePath, parent_thread_id: "someone-else" })],
      2_000,
    );

    const sessions = await readLiveSessions();
    const codex = sessions.find((session) => session.provider === "codex");

    expect(codex?.sessionId).toBe("parent-thread");
    expect(codex?.runningSubtasks).toEqual([
      expect.objectContaining({ id: "child-thread", name: "Fermat" }),
    ]);
  });

  it("세션이 지금 하고 있는 작업으로 가장 최근 사용자 요청을 쓴다", async () => {
    mockTmuxPanes([]);
    await writeJsonLines(claudeSessionFile("session-a"), [
      { type: "user", sessionId: "session-a", cwd: worktreePath, message: { role: "user", content: [{ type: "text", text: "처음 요청" }] } },
      { type: "assistant", sessionId: "session-a", message: { role: "assistant", content: [{ type: "text", text: "답변" }] } },
      { type: "user", sessionId: "session-a", cwd: worktreePath, message: { role: "user", content: [{ type: "text", text: "지금 하고 있는 요청" }] } },
    ], 1_000);

    const sessions = await readLiveSessions();

    expect(sessions.find((session) => session.provider === "claude")?.currentTask).toBe("지금 하고 있는 요청");
  });

  it("Codex 세션도 rollout의 마지막 사용자 입력을 작업으로 쓴다", async () => {
    mockTmuxPanes([]);
    const codexSessions = path.join(tempHome, ".codex", "sessions", "2026", "08", "10");
    await writeJsonLines(path.join(codexSessions, "rollout-parent.jsonl"), [
      { type: "session_meta", payload: { id: "parent-thread", cwd: worktreePath } },
      { type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_text", text: "코덱스 첫 요청" }] } },
      { type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_text", text: "코덱스 최근 요청" }] } },
    ], 1_000);

    const sessions = await readLiveSessions();

    expect(sessions.find((session) => session.provider === "codex")?.currentTask).toBe("코덱스 최근 요청");
  });

  it("세션 기록도 pane도 없는 provider는 목록에서 뺀다", async () => {
    mockTmuxPanes([]);

    expect(await readLiveSessions()).toEqual([]);
  });
});

describe("worktree별 pane 필터", () => {
  const pane = (worktreePath: string): RunningAgentPane => ({
    provider: "claude",
    worktreePath,
    sessionName: "kanvibe-task",
    windowId: "@1",
    windowName: "claude",
  });

  it("worktree 자신과 그 하위 경로에서 실행한 에이전트를 남긴다", () => {
    const panes = [pane("/repo/task"), pane("/repo/task/src"), pane("/repo/other")];

    expect(filterPanesByWorktree(panes, "/repo/task").map((entry) => entry.worktreePath))
      .toEqual(["/repo/task", "/repo/task/src"]);
  });

  it("경로 앞부분만 같은 다른 worktree는 제외한다", () => {
    expect(filterPanesByWorktree([pane("/repo/task-2")], "/repo/task")).toEqual([]);
  });

  it("worktree를 모르면 아무것도 남기지 않는다", () => {
    expect(filterPanesByWorktree([pane("/repo/task")], null)).toEqual([]);
  });
});
