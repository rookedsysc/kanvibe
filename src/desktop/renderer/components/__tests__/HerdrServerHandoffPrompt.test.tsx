import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import HerdrServerHandoffPrompt from "@/desktop/renderer/components/HerdrServerHandoffPrompt";

const { claimHerdrServerHandoffPrompt, replaceHerdrServer } = vi.hoisted(() => ({
  claimHerdrServerHandoffPrompt: vi.fn(),
  replaceHerdrServer: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/sessionDependency", () => ({ claimHerdrServerHandoffPrompt, replaceHerdrServer }));
vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string, values?: Record<string, unknown>) => `${namespace}.${key}:${values?.host}`,
}));

let boardEventListener: (event: unknown) => void;
const HANDOFF_REQUIRED_EVENT = { type: "herdr-server-handoff-required", sshHost: "dev", herdrSession: "kanvibe" };

async function emitBoardEvent(event: unknown) {
  await act(async () => { boardEventListener(event); });
}

describe("HerdrServerHandoffPrompt", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.kanvibeDesktop = {
      onBoardEvent: vi.fn((listener) => { boardEventListener = listener; return () => {}; }),
    } as never;
    claimHerdrServerHandoffPrompt.mockResolvedValue(true);
    replaceHerdrServer.mockResolvedValue(undefined);
  });

  it("교체가 필요하다는 알림을 받으면 호스트를 밝혀 묻고, 승인하면 그 서버를 교체한다", async () => {
    // Given
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<HerdrServerHandoffPrompt />);

    // When
    await emitBoardEvent(HANDOFF_REQUIRED_EVENT);

    // Then
    expect(claimHerdrServerHandoffPrompt).toHaveBeenCalledWith("dev", "kanvibe");
    expect(confirm).toHaveBeenCalledWith("common.sessionDependency.herdrServerHandoffPrompt:dev");
    expect(replaceHerdrServer).toHaveBeenCalledWith("dev", "kanvibe");
  });

  it("거절하면 서버를 교체하지 않는다", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<HerdrServerHandoffPrompt />);

    await emitBoardEvent(HANDOFF_REQUIRED_EVENT);

    expect(replaceHerdrServer).not.toHaveBeenCalled();
  });

  it("이미 물어본 서버거나 다른 창이 먼저 물었으면 다시 묻지 않는다", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    claimHerdrServerHandoffPrompt.mockResolvedValue(false);
    render(<HerdrServerHandoffPrompt />);

    await emitBoardEvent(HANDOFF_REQUIRED_EVENT);

    expect(confirm).not.toHaveBeenCalled();
    expect(replaceHerdrServer).not.toHaveBeenCalled();
  });

  it("다른 보드 알림에는 반응하지 않는다", async () => {
    render(<HerdrServerHandoffPrompt />);

    await emitBoardEvent({ type: "herdr-machine-registration-failed", sshHost: "dev", herdrSession: "kanvibe", reason: "x" });

    expect(claimHerdrServerHandoffPrompt).not.toHaveBeenCalled();
  });
});
