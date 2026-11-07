import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  getHookServerUrl: vi.fn(),
  readTextFile: vi.fn(),
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: mocks.execGit,
}));

vi.mock("@/lib/hookEndpoint", () => ({
  getHookServerUrl: mocks.getHookServerUrl,
}));

vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/hostFileAccess")>()),
  readTextFile: mocks.readTextFile,
}));

const OWN_URL = "http://10.0.0.2:9736";
const PEER_URL = "http://10.0.0.7:9736";

function buildTargetsContent(targets: Array<{ url: string; taskId: string; token?: string }>): string {
  return JSON.stringify({ schemaVersion: 1, targets });
}

describe("taskTargetPropagationService", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.getHookServerUrl.mockResolvedValue(OWN_URL);
  });

  it("다른 client가 없는 worktree에서는 task CLI를 부르지 않는다", async () => {
    mocks.readTextFile.mockResolvedValue(buildTargetsContent([{ url: OWN_URL, taskId: "task-own", token: "own" }]));
    const { propagateTaskDeletionToPeers } = await import("@/desktop/main/services/taskTargetPropagationService");

    const results = await propagateTaskDeletionToPeers("/remote/repo__worktrees/feat", "devbox");

    expect(results).toEqual([]);
    expect(mocks.execGit).not.toHaveBeenCalled();
  });

  it("삭제는 worktree 호스트에서 task CLI로 자기 자신을 뺀 client에 보내고 결과를 client별로 돌려준다", async () => {
    mocks.readTextFile.mockResolvedValue(buildTargetsContent([
      { url: OWN_URL, taskId: "task-own", token: "own" },
      { url: PEER_URL, taskId: "task-peer", token: "peer" },
      { url: "http://10.0.0.9:9736", taskId: "task-offline", token: "offline" },
    ]));
    mocks.execGit.mockResolvedValue([
      "noise",
      `__KANVIBE_PEER_RESULT__\t${PEER_URL}\t200\t{"success":true}`,
      "__KANVIBE_PEER_RESULT__\thttp://10.0.0.9:9736\t000\t연결할 수 없습니다",
      "",
    ].join("\n"));
    const { propagateTaskDeletionToPeers } = await import("@/desktop/main/services/taskTargetPropagationService");

    const results = await propagateTaskDeletionToPeers("/remote/repo__worktrees/feat", "devbox");

    expect(mocks.readTextFile).toHaveBeenCalledWith("/remote/repo__worktrees/feat/.kanvibe/targets.json", "devbox");
    expect(mocks.execGit).toHaveBeenCalledWith(
      expect.stringMatching(new RegExp(`^bash -c .* '/remote/repo__worktrees/feat/\\.kanvibe/task\\.sh' 'propagate-delete' '--exclude-url' '${OWN_URL}'$`, "s")),
      "devbox",
      { timeoutMs: expect.any(Number) },
    );
    expect(results).toEqual([
      { url: PEER_URL, isSuccess: true, httpStatus: "200", message: "{\"success\":true}" },
      { url: "http://10.0.0.9:9736", isSuccess: false, httpStatus: "000", message: "연결할 수 없습니다" },
    ]);
  });

  it("등록 전파는 프로젝트 루트의 task CLI로 새 브랜치를 넘긴다", async () => {
    mocks.readTextFile.mockResolvedValue(buildTargetsContent([{ url: PEER_URL, taskId: "root-peer", token: "peer" }]));
    mocks.execGit.mockResolvedValue(`__KANVIBE_PEER_RESULT__\t${PEER_URL}\t200\tok\n`);
    const { propagateTaskRegistrationToPeers } = await import("@/desktop/main/services/taskTargetPropagationService");

    await propagateTaskRegistrationToPeers("/repo", "feat/new", null);

    expect(mocks.execGit).toHaveBeenCalledWith(
      expect.stringMatching(new RegExp(`^bash -c .* '/repo/\\.kanvibe/task\\.sh' 'propagate-register' '--exclude-url' '${OWN_URL}' '--branch' 'feat/new'$`, "s")),
      null,
      { timeoutMs: expect.any(Number) },
    );
  });

  it("task CLI를 실행하지 못하면 예외 대신 모든 상대 client를 실패로 돌려준다", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    mocks.readTextFile.mockResolvedValue(buildTargetsContent([{ url: PEER_URL, taskId: "task-peer", token: "peer" }]));
    mocks.execGit.mockRejectedValue(new Error("bash: task.sh: No such file or directory"));
    const { propagateTaskDeletionToPeers } = await import("@/desktop/main/services/taskTargetPropagationService");

    const results = await propagateTaskDeletionToPeers("/repo__worktrees/feat", null);

    expect(results).toEqual([
      { url: PEER_URL, isSuccess: false, httpStatus: "000", message: "bash: task.sh: No such file or directory" },
    ]);
  });
});
