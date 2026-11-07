import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  buildTaskCliToken: vi.fn(),
  ensureAiToolPatternsExcluded: vi.fn(),
  upsertKanvibeHookTarget: vi.fn(),
}));

vi.mock("@/desktop/main/services/taskCliTokenService", () => ({
  buildTaskCliToken: mocks.buildTaskCliToken,
}));

vi.mock("@/lib/gitExclude", () => ({
  ensureAiToolPatternsExcluded: mocks.ensureAiToolPatternsExcluded,
}));

vi.mock("@/lib/kanvibeProjectState", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/kanvibeProjectState")>()),
  upsertKanvibeHookTarget: mocks.upsertKanvibeHookTarget,
}));

const { registerKanvibeHookTarget, verifyHookTargetRegistration } = await import("@/lib/hookTargetRegistration");

const TARGETS_CONTENT = JSON.stringify({
  schemaVersion: 1,
  targets: [
    { url: "http://127.0.0.1:9736", taskId: "task-local", token: "token-local" },
    { url: "http://10.0.0.5:9736", taskId: "task-remote", token: "token-remote" },
  ],
});

/** task CLI 토큰이 생기기 전에 설치된 targets.json */
const TOKENLESS_TARGETS_CONTENT = JSON.stringify({
  schemaVersion: 1,
  targets: [
    { url: "http://127.0.0.1:9736", taskId: "task-local" },
    { url: "http://10.0.0.5:9736", taskId: "task-remote" },
  ],
});

describe("hookTargetRegistration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.buildTaskCliToken.mockResolvedValue("signed-token");
  });

  it("client를 등록할 때 그 task의 CLI 토큰을 함께 기록한다", async () => {
    await registerKanvibeHookTarget("/repo", "task-1", "http://127.0.0.1:9736", "remote-host");

    expect(mocks.buildTaskCliToken).toHaveBeenCalledWith("task-1");
    expect(mocks.upsertKanvibeHookTarget).toHaveBeenCalledWith(
      "/repo",
      { url: "http://127.0.0.1:9736", taskId: "task-1", token: "signed-token" },
      "remote-host",
    );
  });

  it("CLI 토큰 없이 등록된 예전 설치는 재설치가 필요하도록 미설치로 본다", () => {
    expect(verifyHookTargetRegistration(TOKENLESS_TARGETS_CONTENT, "task-local", "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: "http://127.0.0.1:9736",
    });
    expect(verifyHookTargetRegistration(TOKENLESS_TARGETS_CONTENT, "task-remote", null)).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: "http://10.0.0.5:9736",
    });
  });

  it("현재 client url과 taskId가 모두 등록되어야 설치된 것으로 본다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-local", "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: true,
      registeredHookTargetUrl: "http://127.0.0.1:9736",
    });
  });

  it("다른 client가 같은 task를 등록했더라도 내 client url이 없으면 미설치로 본다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-remote", "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: "http://10.0.0.5:9736",
    });
  });

  it("등록되지 않은 task는 미설치로 본다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-unknown", "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: null,
    });
  });

  it("hook 서버 주소를 확정하지 못하면 taskId 등록 여부만으로 판정한다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-remote", null)).toEqual({
      hasRegisteredHookTarget: true,
      registeredHookTargetUrl: "http://10.0.0.5:9736",
    });
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-unknown", null)).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: null,
    });
  });

  it("확인할 task가 없으면 등록 여부를 판정하지 않는다", () => {
    expect(verifyHookTargetRegistration("", undefined, "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: true,
      registeredHookTargetUrl: null,
    });
  });
});
