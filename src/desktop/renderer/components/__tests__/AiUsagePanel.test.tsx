import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AiUsagePanel from "../AiUsagePanel";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, string>) => (
    values ? `${key}:${Object.values(values).join(",")}` : key
  ),
}));

const { mockUseAiUsage } = vi.hoisted(() => ({ mockUseAiUsage: vi.fn() }));

vi.mock("@/desktop/renderer/hooks/useAiUsage", () => ({ useAiUsage: mockUseAiUsage }));

function createSnapshot(): AiUsageSnapshot {
  return {
    fetchedAt: "2026-08-10T06:00:00.000Z",
    providers: [
      {
        provider: "claude",
        status: "ok",
        planName: null,
        windows: [
          { kind: "session", modelName: null, usedPercent: 22, resetsAt: null },
          { kind: "weekly", modelName: null, usedPercent: 95, resetsAt: null },
        ],
        reason: null,
        fetchedAt: "2026-08-10T06:00:00.000Z",
      },
      {
        provider: "codex",
        status: "ok",
        planName: "pro",
        windows: [{ kind: "weekly", modelName: null, usedPercent: 35, resetsAt: null }],
        reason: null,
        fetchedAt: "2026-08-10T06:00:00.000Z",
      },
      {
        provider: "gemini",
        status: "unavailable",
        planName: null,
        windows: [],
        reason: "missing-credentials",
        fetchedAt: "2026-08-10T06:00:00.000Z",
      },
    ],
  };
}

function renderPanel(overrides: Partial<ReturnType<typeof mockUseAiUsage>> = {}) {
  const refresh = vi.fn();
  mockUseAiUsage.mockReturnValue({
    snapshot: createSnapshot(),
    isLoading: false,
    hasFailed: false,
    refresh,
    ...overrides,
  });

  render(<AiUsagePanel isOpen />);
  return refresh;
}

describe("AiUsagePanel", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("provider마다 사용한 비율과 등급을 보여준다", () => {
    renderPanel();

    expect(screen.getByText("22%")).toBeDefined();
    expect(screen.getByText("95%")).toBeDefined();
    expect(screen.getByText("35%")).toBeDefined();
    expect(screen.getByText("pro")).toBeDefined();
  });

  it("한도에 가까운 창은 강조색이 아니라 심각도 색으로 칠한다", () => {
    renderPanel();

    const claudeCard = screen.getByTestId("ai-usage-provider-claude");
    const bars = claudeCard.querySelectorAll("[style*='width']");

    expect(bars[0].className).toContain("bg-brand-primary");
    expect(bars[1].className).toContain("bg-status-error");
  });

  it("자격증명이 없는 provider는 막대 대신 사유를 보여준다", () => {
    renderPanel();

    const geminiCard = screen.getByTestId("ai-usage-provider-gemini");

    expect(geminiCard.textContent).toContain("reasons.missing-credentials");
    expect(geminiCard.querySelector("[style*='width']")).toBeNull();
  });

  it("새로고침 버튼은 조회를 다시 요청한다", () => {
    const refresh = renderPanel();

    fireEvent.click(screen.getByTestId("ai-usage-refresh"));

    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("조회 중에는 새로고침을 막아 중복 호출을 만들지 않는다", () => {
    renderPanel({ isLoading: true });

    expect(screen.getByTestId("ai-usage-refresh").hasAttribute("disabled")).toBe(true);
  });

  it("스냅샷을 아예 못 받으면 실패를 알린다", () => {
    renderPanel({ snapshot: null, hasFailed: true });

    expect(screen.getByText("snapshotFailed")).toBeDefined();
  });
});
