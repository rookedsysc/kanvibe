import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClaudeUsage } from "@/lib/aiUsage/readClaudeUsage";

const { mockGetHomeDirectory, mockReadTextFile } = vi.hoisted(() => ({
  mockGetHomeDirectory: vi.fn(),
  mockReadTextFile: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  getHomeDirectory: mockGetHomeDirectory,
  readTextFile: mockReadTextFile,
}));

function createCredentialsJson(accessToken: string): string {
  return JSON.stringify({ claudeAiOauth: { accessToken, refreshToken: "refresh-token" } });
}

describe("readClaudeUsage", () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("fetch", fetchMock);
    mockGetHomeDirectory.mockResolvedValue("/home/tester");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("5시간 창과 7일 창을 각각 session·weekly 창으로 매핑한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        five_hour: { utilization: 22, resets_at: "2026-08-10T06:20:00.000Z" },
        seven_day: { utilization: 61, resets_at: "2026-08-13T02:00:00.000Z" },
      }),
    });

    const result = await readClaudeUsage();

    expect(result.status).toBe("ok");
    expect(result.windows).toEqual([
      { kind: "session", modelName: null, usedPercent: 22, resetsAt: "2026-08-10T06:20:00.000Z" },
      { kind: "weekly", modelName: null, usedPercent: 61, resetsAt: "2026-08-13T02:00:00.000Z" },
    ]);
  });

  it("자격증명 파일이 없으면 네트워크를 호출하지 않고 unavailable을 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue("");

    const result = await readClaudeUsage();

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("missing-credentials");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("401 응답은 재로그인이 필요한 부재로 분류한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("expired-token"));
    fetchMock.mockResolvedValue({ ok: false, status: 401, json: async () => ({}) });

    const result = await readClaudeUsage();

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("expired-credentials");
  });

  it("두 창이 모두 비어 있으면 응답 형식이 바뀐 것으로 보고 error를 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    fetchMock.mockResolvedValue({ ok: true, status: 200, json: async () => ({}) });

    const result = await readClaudeUsage();

    expect(result.status).toBe("error");
    expect(result.reason).toBe("empty-response");
  });
});
