import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readGeminiUsage } from "@/lib/aiUsage/readGeminiUsage";

const {
  mockGetHomeDirectory,
  mockReadTextFile,
  mockResolveGeminiOAuthClient,
  mockRefreshGeminiAccessToken,
  mockLoadGeminiProjectId,
} = vi.hoisted(() => ({
  mockGetHomeDirectory: vi.fn(),
  mockReadTextFile: vi.fn(),
  mockResolveGeminiOAuthClient: vi.fn(),
  mockRefreshGeminiAccessToken: vi.fn(),
  mockLoadGeminiProjectId: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  getHomeDirectory: mockGetHomeDirectory,
  readTextFile: mockReadTextFile,
}));

vi.mock("@/lib/aiUsage/geminiOAuthClient", () => ({
  resolveGeminiOAuthClient: mockResolveGeminiOAuthClient,
  refreshGeminiAccessToken: mockRefreshGeminiAccessToken,
  loadGeminiProjectId: mockLoadGeminiProjectId,
}));

const ONE_HOUR_MS = 60 * 60 * 1000;

function createCredentialsJson(expiryDate: number): string {
  return JSON.stringify({
    access_token: "access-token",
    refresh_token: "refresh-token",
    expiry_date: expiryDate,
  });
}

describe("readGeminiUsage", () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("fetch", fetchMock);
    mockGetHomeDirectory.mockResolvedValue("/home/tester");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("Gemini CLI 자격증명이 없으면 unavailable을 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue("");

    const result = await readGeminiUsage();

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("missing-credentials");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("토큰이 만료됐는데 Gemini CLI 설치본을 못 찾으면 갱신을 포기하고 안내한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson(Date.now() - ONE_HOUR_MS));
    mockResolveGeminiOAuthClient.mockResolvedValue(null);

    const result = await readGeminiUsage();

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("gemini-cli-not-found");
    expect(mockRefreshGeminiAccessToken).not.toHaveBeenCalled();
  });

  it("프로젝트를 알아내지 못하면 쿼터를 호출하지 않고 error를 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson(Date.now() + ONE_HOUR_MS));
    mockLoadGeminiProjectId.mockResolvedValue(null);

    const result = await readGeminiUsage();

    expect(result.status).toBe("error");
    expect(result.reason).toBe("fetch-failed");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("남은 비율을 사용한 비율로 뒤집고 모델 id를 표시 이름으로 바꾼다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson(Date.now() + ONE_HOUR_MS));
    mockLoadGeminiProjectId.mockResolvedValue("project-id");
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        buckets: [
          {
            remainingFraction: 0.25,
            modelId: "gemini-2.5-pro",
            resetTime: "2026-08-10T07:00:00.000Z",
          },
        ],
      }),
    });

    const result = await readGeminiUsage();

    expect(result.status).toBe("ok");
    expect(result.windows).toEqual([
      {
        kind: "model",
        modelName: "Pro",
        usedPercent: 75,
        resetsAt: "2026-08-10T07:00:00.000Z",
      },
    ]);
  });

  it("표에 없는 모델 id는 접두사를 떼고 읽을 수 있는 이름으로 만든다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson(Date.now() + ONE_HOUR_MS));
    mockLoadGeminiProjectId.mockResolvedValue("project-id");
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        buckets: [
          {
            remainingFraction: 1,
            modelId: "gemini-4.0-ultra",
            resetTime: "2026-08-10T07:00:00.000Z",
          },
        ],
      }),
    });

    const result = await readGeminiUsage();

    expect(result.windows[0].modelName).toBe("4.0 Ultra");
    expect(result.windows[0].usedPercent).toBe(0);
  });
});
