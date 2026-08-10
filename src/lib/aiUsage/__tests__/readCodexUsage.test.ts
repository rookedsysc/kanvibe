import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readCodexUsage } from "@/lib/aiUsage/readCodexUsage";

const { mockGetHomeDirectory, mockReadTextFile } = vi.hoisted(() => ({
  mockGetHomeDirectory: vi.fn(),
  mockReadTextFile: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  getHomeDirectory: mockGetHomeDirectory,
  readTextFile: mockReadTextFile,
}));

function createAuthJson(): string {
  return JSON.stringify({
    tokens: { access_token: "access-token", account_id: "account-id" },
  });
}

describe("readCodexUsage", () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("fetch", fetchMock);
    mockGetHomeDirectory.mockResolvedValue("/home/tester");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("창 길이가 604800초인 primary_window를 session이 아니라 weekly로 분류한다", async () => {
    mockReadTextFile.mockResolvedValue(createAuthJson());
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        plan_type: "pro",
        rate_limit: {
          primary_window: {
            used_percent: 35,
            limit_window_seconds: 604800,
            reset_at: 1786840544,
          },
          secondary_window: null,
        },
      }),
    });

    const result = await readCodexUsage();

    expect(result.status).toBe("ok");
    expect(result.planName).toBe("pro");
    expect(result.windows).toEqual([
      {
        kind: "weekly",
        modelName: null,
        usedPercent: 35,
        resetsAt: new Date(1786840544 * 1000).toISOString(),
      },
    ]);
  });

  it("5시간 창과 7일 창이 함께 오면 창 길이대로 둘 다 매핑한다", async () => {
    mockReadTextFile.mockResolvedValue(createAuthJson());
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        plan_type: "plus",
        rate_limit: {
          primary_window: { used_percent: 10, limit_window_seconds: 18000, reset_at: 1786800000 },
          secondary_window: { used_percent: 80, limit_window_seconds: 604800, reset_at: 1786900000 },
        },
      }),
    });

    const result = await readCodexUsage();

    expect(result.windows.map((window) => window.kind)).toEqual(["session", "weekly"]);
    expect(result.windows.map((window) => window.usedPercent)).toEqual([10, 80]);
  });

  it("액세스 토큰이 없으면 네트워크를 호출하지 않고 unavailable을 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({ auth_mode: "apikey", tokens: null }));

    const result = await readCodexUsage();

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("missing-credentials");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("계정 식별자를 헤더로 함께 보낸다", async () => {
    mockReadTextFile.mockResolvedValue(createAuthJson());
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        plan_type: "pro",
        rate_limit: { primary_window: { used_percent: 1, limit_window_seconds: 604800, reset_at: 1 } },
      }),
    });

    await readCodexUsage();

    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers["ChatGPT-Account-Id"]).toBe("account-id");
  });
});
