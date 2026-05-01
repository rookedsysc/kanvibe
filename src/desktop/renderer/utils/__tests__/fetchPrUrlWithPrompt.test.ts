import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  fetchAndSavePrUrl: vi.fn(),
  ensureGitHubCliWithPrompt: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  fetchAndSavePrUrl: mocks.fetchAndSavePrUrl,
}));

vi.mock("@/desktop/renderer/utils/githubCliDependencyPrompt", () => ({
  ensureGitHubCliWithPrompt: mocks.ensureGitHubCliWithPrompt,
}));

describe("fetchPrUrlWithPrompt", () => {
  const tCommon = (key: string) => key;

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("원격 task에서 gh 설치를 거절하면 PR 조회를 중단한다", async () => {
    mocks.ensureGitHubCliWithPrompt.mockResolvedValue(false);
    const { fetchPrUrlWithPrompt } = await import("@/desktop/renderer/utils/fetchPrUrlWithPrompt");

    const result = await fetchPrUrlWithPrompt({
      id: "task-1",
      branchName: "fix/wtf",
      prUrl: null,
      sshHost: "remote-a",
      project: { sshHost: "remote-a" },
    } as never, tCommon);

    expect(result).toBeNull();
    expect(mocks.ensureGitHubCliWithPrompt).toHaveBeenCalledWith("remote-a", tCommon);
    expect(mocks.fetchAndSavePrUrl).not.toHaveBeenCalled();
  });

  it("원격 task에서 gh 설치가 준비되면 PR 조회를 진행한다", async () => {
    mocks.ensureGitHubCliWithPrompt.mockResolvedValue(true);
    mocks.fetchAndSavePrUrl.mockResolvedValue("https://github.com/kanvibe/kanvibe/pull/165");
    const { fetchPrUrlWithPrompt } = await import("@/desktop/renderer/utils/fetchPrUrlWithPrompt");

    const result = await fetchPrUrlWithPrompt({
      id: "task-2",
      branchName: "fix/wtf",
      prUrl: null,
      sshHost: "remote-a",
      project: { sshHost: "remote-a" },
    } as never, tCommon);

    expect(result).toBe("https://github.com/kanvibe/kanvibe/pull/165");
    expect(mocks.fetchAndSavePrUrl).toHaveBeenCalledWith("task-2");
  });

  it("로컬 task는 프롬프트 없이 바로 PR 조회를 진행한다", async () => {
    mocks.fetchAndSavePrUrl.mockResolvedValue("https://github.com/kanvibe/kanvibe/pull/1");
    const { fetchPrUrlWithPrompt } = await import("@/desktop/renderer/utils/fetchPrUrlWithPrompt");

    const result = await fetchPrUrlWithPrompt({
      id: "task-3",
      branchName: "main",
      prUrl: null,
      sshHost: null,
      project: { sshHost: null },
    } as never, tCommon);

    expect(result).toBe("https://github.com/kanvibe/kanvibe/pull/1");
    expect(mocks.ensureGitHubCliWithPrompt).not.toHaveBeenCalled();
    expect(mocks.fetchAndSavePrUrl).toHaveBeenCalledWith("task-3");
  });
});
