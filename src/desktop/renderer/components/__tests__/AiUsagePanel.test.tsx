import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import AiUsagePanel from "../AiUsagePanel";
import type { AiUsageAccountResult, AiUsageSnapshot } from "@/lib/aiUsage/types";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, string>) => (
    values ? `${key}:${Object.values(values).join(",")}` : key
  ),
}));

const { mockUseAiUsage } = vi.hoisted(() => ({ mockUseAiUsage: vi.fn() }));

vi.mock("@/desktop/renderer/hooks/useAiUsage", () => ({ useAiUsage: mockUseAiUsage }));

function createClaudeAccount(overrides: Partial<AiUsageAccountResult> = {}): AiUsageAccountResult {
  return {
    provider: "claude",
    accountId: "personal",
    label: "me@example.com",
    status: "ok",
    planName: null,
    windows: [
      { kind: "session", modelName: null, usedPercent: 22, resetsAt: null },
      { kind: "weekly", modelName: null, usedPercent: 95, resetsAt: null },
    ],
    reason: null,
    fetchedAt: "2026-08-10T06:00:00.000Z",
    ...overrides,
  };
}

function createSnapshot(accounts?: AiUsageAccountResult[]): AiUsageSnapshot {
  return {
    fetchedAt: "2026-08-10T06:00:00.000Z",
    accounts: accounts ?? [
      createClaudeAccount(),
      {
        provider: "codex",
        accountId: "codex-seat",
        label: "codex@example.com",
        status: "ok",
        planName: "pro",
        windows: [{ kind: "weekly", modelName: null, usedPercent: 35, resetsAt: null }],
        reason: null,
        fetchedAt: "2026-08-10T06:00:00.000Z",
      },
      {
        provider: "gemini",
        accountId: "gemini",
        label: "gemini",
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
    isRefreshing: false,
    hasFailed: false,
    refresh,
    ...overrides,
  });

  // 계정 화면으로 가는 링크가 붙어 패널도 라우터 컨텍스트를 요구한다
  render(
    <MemoryRouter initialEntries={["/ko"]}>
      <AiUsagePanel isOpen />
    </MemoryRouter>,
  );
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

  it("직전 값을 이어 붙인 카드는 막대와 실패 사유를 함께 보여준다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount({ status: "error", reason: "rate-limited" }),
      ]),
    });

    const claudeCard = screen.getByTestId("ai-usage-provider-claude");

    expect(screen.getByText("22%")).toBeDefined();
    expect(claudeCard.textContent).toContain("reasons.rate-limited");
  });

  it("이어 붙인 값에는 언제 기준인지 함께 밝힌다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount({
          status: "error",
          reason: "rate-limited",
          fetchedAt: "2026-08-09T01:00:00.000Z",
        }),
      ]),
    });

    expect(screen.getByTestId("ai-usage-carried").textContent).toContain("carriedValues:");
  });

  it("조회에 성공한 카드에는 이어 붙였다는 표시를 달지 않는다", () => {
    renderPanel();

    expect(screen.queryByTestId("ai-usage-carried")).toBeNull();
  });

  it("계정이 하나뿐이어도 어느 계정의 사용량인지 밝힌다", () => {
    renderPanel();

    expect(screen.getByTestId("ai-usage-provider-claude").textContent).toContain("me@example.com");
  });

  it("등급은 provider가 아니라 계정마다 따로 보여준다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount({ planName: "max" }),
        createClaudeAccount({ accountId: "work", label: "work@example.com", planName: "pro" }),
      ]),
    });

    const accounts = screen.getAllByTestId("ai-usage-account");

    expect(accounts[0].textContent).toContain("max");
    expect(accounts[1].textContent).toContain("pro");
  });

  it("라벨이 provider 이름뿐이면 카드 제목을 되풀이하지 않는다", () => {
    renderPanel();

    const geminiCard = screen.getByTestId("ai-usage-provider-gemini");

    expect(geminiCard.querySelector("[data-testid='ai-usage-account-label']")).toBeNull();
  });

  it("모델별 주간 한도는 7일 한도 아래로 묶어 보여준다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount({
          windows: [
            { kind: "session", modelName: null, usedPercent: 22, resetsAt: null },
            { kind: "weekly", modelName: null, usedPercent: 2, resetsAt: null },
            { kind: "weekly", modelName: "Fable", usedPercent: 4, resetsAt: null },
          ],
        }),
      ]),
    });

    const weeklyScope = screen.getByTestId("ai-usage-scoped-weekly");

    expect(weeklyScope.textContent).toContain("Fable");
    expect(weeklyScope.textContent).not.toContain("weekly");
  });

  it("딸릴 기간 한도가 없는 모델 창은 들여쓰지 않는다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount({
          provider: "gemini",
          windows: [
            { kind: "model", modelName: "Pro", usedPercent: 12, resetsAt: null },
            { kind: "model", modelName: "Flash", usedPercent: 3, resetsAt: null },
          ],
        }),
      ]),
    });

    expect(screen.queryByTestId("ai-usage-scoped-model")).toBeNull();
    expect(screen.getByText("Pro")).toBeDefined();
    expect(screen.getByText("Flash")).toBeDefined();
  });

  it("한 provider에 계정이 여럿이면 어느 계정의 사용량인지 밝힌다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount(),
        createClaudeAccount({
          accountId: "work",
          label: "work@example.com",
          windows: [{ kind: "session", modelName: null, usedPercent: 7, resetsAt: null }],
        }),
      ]),
    });

    const claudeCard = screen.getByTestId("ai-usage-provider-claude");

    expect(claudeCard.textContent).toContain("me@example.com");
    expect(claudeCard.textContent).toContain("work@example.com");
    expect(claudeCard.querySelectorAll("[data-testid='ai-usage-account']")).toHaveLength(2);
  });

  it("저장된 값을 보여주는 동안에는 불러오는 중이 아니라 갱신 중이라고 알린다", () => {
    renderPanel({ isRefreshing: true, isLoading: false });

    expect(screen.getByTestId("ai-usage-refreshing")).toBeDefined();
    expect(screen.queryByText("loading")).toBeNull();
    expect(screen.getByText("22%")).toBeDefined();
  });

  it("보여줄 값이 없을 때만 불러오는 중이라고 표시한다", () => {
    renderPanel({ snapshot: null, isLoading: true, isRefreshing: true });

    expect(screen.getByText("loading")).toBeDefined();
  });

  it("새로고침 버튼은 조회를 다시 요청한다", () => {
    const refresh = renderPanel();

    fireEvent.click(screen.getByTestId("ai-usage-refresh"));

    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("조회 중에는 새로고침을 막아 중복 호출을 만들지 않는다", () => {
    renderPanel({ isRefreshing: true });

    expect(screen.getByTestId("ai-usage-refresh").hasAttribute("disabled")).toBe(true);
  });

  it("스냅샷을 아예 못 받으면 실패를 알린다", () => {
    renderPanel({ snapshot: null, hasFailed: true });

    expect(screen.getByText("snapshotFailed")).toBeDefined();
  });
});

describe("AiUsagePanel 계정 동선", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("로그인으로 풀리는 사유에는 계정 화면으로 가는 길을 함께 보여준다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount({ status: "unavailable", windows: [], reason: "expired-credentials" }),
      ]),
    });

    expect(screen.queryByTestId("ai-usage-manage-accounts")).not.toBeNull();
  });

  it("로그인과 무관한 사유에는 계정 화면으로 보내지 않는다", () => {
    renderPanel({
      snapshot: createSnapshot([
        createClaudeAccount({ status: "error", windows: [], reason: "rate-limited" }),
      ]),
    });

    expect(screen.queryByTestId("ai-usage-manage-accounts")).toBeNull();
  });
});
