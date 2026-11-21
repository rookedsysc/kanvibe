import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/terminal", () => ({ replaceHerdrRemoteServer: vi.fn() }));
vi.mock("@/lib/remoteSessionDependency", () => ({}));

describe("원격 Herdr 서버 교체 확인", () => {
  it("같은 서버는 앱이 실행되는 동안 처음 한 번만 묻고, 다른 서버는 따로 묻는다", async () => {
    const { claimHerdrServerHandoffPrompt } = await import("@/desktop/main/services/sessionDependencyService");

    expect(claimHerdrServerHandoffPrompt("dev", "kanvibe")).toBe(true);
    expect(claimHerdrServerHandoffPrompt("dev", "kanvibe")).toBe(false);
    expect(claimHerdrServerHandoffPrompt("dev", "feature")).toBe(true);
    expect(claimHerdrServerHandoffPrompt("other", "kanvibe")).toBe(true);
  });
});
