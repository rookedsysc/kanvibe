/**
 * @vitest-environment node
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

const { mockAggregateAiUsage, storedSettings, settingsStore } = vi.hoisted(() => ({
  mockAggregateAiUsage: vi.fn(),
  storedSettings: new Map<string, string>(),
  settingsStore: { isWritable: true },
}));

vi.mock("@/lib/aiUsage/aggregateAiUsage", () => ({ aggregateAiUsage: mockAggregateAiUsage }));

vi.mock("@/lib/database", () => ({
  getAppSettingsRepository: async () => ({
    findOne: async ({ where }: { where: { key: string } }) => (
      storedSettings.has(where.key) ? { key: where.key, value: storedSettings.get(where.key) } : null
    ),
    create: (setting: { key: string; value: string }) => setting,
    save: async (setting: { key: string; value: string }) => {
      if (!settingsStore.isWritable) {
        throw new Error("database is locked");
      }
      storedSettings.set(setting.key, setting.value);
      return setting;
    },
  }),
}));

function createSnapshot(usedPercent: number, resetsAt: string | null = null): AiUsageSnapshot {
  return {
    fetchedAt: "2026-08-10T06:00:00.000Z",
    accounts: [
      {
        provider: "claude",
        accountId: "account-uuid",
        label: "me@example.com",
        status: "ok",
        planName: "max",
        windows: [{ kind: "session", modelName: null, usedPercent, resetsAt }],
        reason: null,
        fetchedAt: "2026-08-10T06:00:00.000Z",
      },
    ],
  };
}

/** 429처럼 값을 하나도 얻지 못한 조회. 같은 계정이지만 창이 비어 있다 */
function createRateLimitedSnapshot(fetchedAt: string): AiUsageSnapshot {
  return {
    fetchedAt,
    accounts: [
      {
        provider: "claude",
        accountId: "account-uuid",
        label: "me@example.com",
        status: "error",
        planName: null,
        windows: [],
        reason: "rate-limited",
        fetchedAt,
      },
    ],
  };
}

describe("aiUsageService", () => {
  beforeEach(() => {
    storedSettings.clear();
    settingsStore.isWritable = true;
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("한 번도 조회하지 않았으면 캐시가 비어 있다", async () => {
    const { getCachedAiUsageSnapshot } = await import("@/desktop/main/services/aiUsageService");

    expect(await getCachedAiUsageSnapshot()).toBeNull();
  });

  it("조회한 스냅샷을 앱을 껐다 켜도 남는 설정에 저장한다", async () => {
    mockAggregateAiUsage.mockResolvedValue(createSnapshot(22));
    const { getAiUsageSnapshot, getCachedAiUsageSnapshot } = await import(
      "@/desktop/main/services/aiUsageService"
    );

    await getAiUsageSnapshot();
    const cached = await getCachedAiUsageSnapshot();

    expect(cached?.accounts[0].windows[0].usedPercent).toBe(22);
    expect(cached?.accounts[0].label).toBe("me@example.com");
  });

  it("캐시에는 계정 식별자를 그대로 남기지 않는다", async () => {
    mockAggregateAiUsage.mockResolvedValue(createSnapshot(22));
    const { getAiUsageSnapshot } = await import("@/desktop/main/services/aiUsageService");

    await getAiUsageSnapshot();

    expect([...storedSettings.values()].join()).not.toContain("account-uuid");
  });

  it("다시 조회하면 캐시를 새 결과로 바꾼다", async () => {
    const { getAiUsageSnapshot, getCachedAiUsageSnapshot } = await import(
      "@/desktop/main/services/aiUsageService"
    );

    mockAggregateAiUsage.mockResolvedValue(createSnapshot(22));
    await getAiUsageSnapshot();
    mockAggregateAiUsage.mockResolvedValue(createSnapshot(80));
    await getAiUsageSnapshot();

    expect((await getCachedAiUsageSnapshot())?.accounts[0].windows[0].usedPercent).toBe(80);
  });

  it("조회가 실패하면 직전 값을 이어 붙이되 실패 사유는 지우지 않는다", async () => {
    const { getAiUsageSnapshot } = await import("@/desktop/main/services/aiUsageService");

    mockAggregateAiUsage.mockResolvedValue(createSnapshot(22, "2999-01-01T00:00:00.000Z"));
    await getAiUsageSnapshot();
    mockAggregateAiUsage.mockResolvedValue(createRateLimitedSnapshot("2026-08-10T06:05:00.000Z"));
    const snapshot = await getAiUsageSnapshot();

    expect(snapshot.accounts[0].windows[0].usedPercent).toBe(22);
    expect(snapshot.accounts[0].reason).toBe("rate-limited");
    expect(snapshot.accounts[0].planName).toBe("max");
  });

  it("이어 붙인 값에는 그 값이 만들어진 조회 시각을 남긴다", async () => {
    const { getAiUsageSnapshot } = await import("@/desktop/main/services/aiUsageService");

    mockAggregateAiUsage.mockResolvedValue(createSnapshot(22, "2999-01-01T00:00:00.000Z"));
    await getAiUsageSnapshot();
    mockAggregateAiUsage.mockResolvedValue(createRateLimitedSnapshot("2026-08-10T06:05:00.000Z"));
    const snapshot = await getAiUsageSnapshot();

    expect(snapshot.accounts[0].fetchedAt).toBe("2026-08-10T06:00:00.000Z");
    expect(snapshot.fetchedAt).toBe("2026-08-10T06:05:00.000Z");
  });

  it("이미 초기화된 창의 값은 이어 붙이지 않는다", async () => {
    const { getAiUsageSnapshot } = await import("@/desktop/main/services/aiUsageService");

    mockAggregateAiUsage.mockResolvedValue(createSnapshot(22, "2026-08-10T06:01:00.000Z"));
    await getAiUsageSnapshot();
    mockAggregateAiUsage.mockResolvedValue(createRateLimitedSnapshot("2026-08-10T06:05:00.000Z"));
    const snapshot = await getAiUsageSnapshot();

    expect(snapshot.accounts[0].windows).toEqual([]);
    expect(snapshot.accounts[0].reason).toBe("rate-limited");
  });

  it("이어 붙일 직전 값이 없으면 실패 결과를 그대로 둔다", async () => {
    mockAggregateAiUsage.mockResolvedValue(createRateLimitedSnapshot("2026-08-10T06:05:00.000Z"));
    const { getAiUsageSnapshot } = await import("@/desktop/main/services/aiUsageService");

    const snapshot = await getAiUsageSnapshot();

    expect(snapshot.accounts[0].windows).toEqual([]);
  });

  it("캐시 저장이 실패해도 조회 결과는 그대로 돌려준다", async () => {
    mockAggregateAiUsage.mockResolvedValue(createSnapshot(22));
    settingsStore.isWritable = false;
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { getAiUsageSnapshot } = await import("@/desktop/main/services/aiUsageService");

    const snapshot = await getAiUsageSnapshot();

    expect(snapshot.accounts[0].windows[0].usedPercent).toBe(22);
    expect(errorSpy).toHaveBeenCalled();
  });
});
