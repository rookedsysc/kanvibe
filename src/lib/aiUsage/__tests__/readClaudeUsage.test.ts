import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClaudeUsage } from "@/lib/aiUsage/readClaudeUsage";
import type { AiUsageAccount } from "@/lib/aiUsage/types";

const {
  mockReadTextFile,
  mockIsClaudeTokenExpiring,
  mockRefreshClaudeCredentials,
  mockWriteCredentialsAtomically,
} = vi.hoisted(() => ({
  mockReadTextFile: vi.fn(),
  mockIsClaudeTokenExpiring: vi.fn(),
  mockRefreshClaudeCredentials: vi.fn(),
  mockWriteCredentialsAtomically: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({ readTextFile: mockReadTextFile }));

vi.mock("@/lib/aiUsage/claudeOAuthRefresh", () => ({
  isClaudeTokenExpiring: mockIsClaudeTokenExpiring,
  refreshClaudeCredentials: mockRefreshClaudeCredentials,
}));

vi.mock("@/lib/aiUsage/atomicCredentialsWrite", () => ({
  writeCredentialsAtomically: mockWriteCredentialsAtomically,
}));

const ACCOUNT: AiUsageAccount = {
  provider: "claude",
  accountId: "account-uuid",
  label: "me@example.com",
  configDir: "/home/tester/.claude",
};

function createCredentialsJson(accessToken: string): string {
  return JSON.stringify({ claudeAiOauth: { accessToken, refreshToken: "refresh-token" } });
}

const fetchMock = vi.fn();

function stubUsageResponse(): void {
  fetchMock.mockResolvedValue({
    ok: true,
    status: 200,
    json: async () => ({ five_hour: { utilization: 5, resets_at: null } }),
  });
}

describe("readClaudeUsage", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", fetchMock);
    mockIsClaudeTokenExpiring.mockReturnValue(false);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("계정의 config dir에서 자격증명을 읽고 결과에 계정 정보를 담는다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    stubUsageResponse();

    const result = await readClaudeUsage(ACCOUNT);

    expect(mockReadTextFile).toHaveBeenCalledWith(path.join(ACCOUNT.configDir, ".credentials.json"));
    expect(result.accountId).toBe("account-uuid");
    expect(result.label).toBe("me@example.com");
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

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("ok");
    expect(result.windows).toEqual([
      { kind: "session", modelName: null, usedPercent: 22, resetsAt: "2026-08-10T06:20:00.000Z" },
      { kind: "weekly", modelName: null, usedPercent: 61, resetsAt: "2026-08-13T02:00:00.000Z" },
    ]);
  });

  it("만료가 임박하면 갱신한 자격증명을 먼저 되쓰고 새 액세스 토큰으로 조회한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("stale-token"));
    mockIsClaudeTokenExpiring.mockReturnValue(true);
    mockRefreshClaudeCredentials.mockResolvedValue(createCredentialsJson("fresh-token"));
    stubUsageResponse();

    await readClaudeUsage(ACCOUNT);

    expect(mockWriteCredentialsAtomically).toHaveBeenCalledWith(
      path.join(ACCOUNT.configDir, ".credentials.json"),
      createCredentialsJson("fresh-token"),
    );
    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers.Authorization).toBe("Bearer fresh-token");
  });

  it("갱신이 실패하면 되쓰지 않고 저장된 토큰으로 조회를 이어간다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("stale-token"));
    mockIsClaudeTokenExpiring.mockReturnValue(true);
    mockRefreshClaudeCredentials.mockResolvedValue(null);
    stubUsageResponse();

    await readClaudeUsage(ACCOUNT);

    expect(mockWriteCredentialsAtomically).not.toHaveBeenCalled();
    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers.Authorization).toBe("Bearer stale-token");
  });

  it("되쓰기가 실패해도 갱신한 토큰으로 조회는 계속한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("stale-token"));
    mockIsClaudeTokenExpiring.mockReturnValue(true);
    mockRefreshClaudeCredentials.mockResolvedValue(createCredentialsJson("fresh-token"));
    mockWriteCredentialsAtomically.mockRejectedValue(new Error("read-only file system"));
    vi.spyOn(console, "error").mockImplementation(() => {});
    stubUsageResponse();

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("ok");
    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers.Authorization).toBe("Bearer fresh-token");
  });

  it("자격증명 파일이 없으면 네트워크를 호출하지 않고 unavailable을 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue("");

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("missing-credentials");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("401 응답은 재로그인이 필요한 부재로 분류한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("expired-token"));
    fetchMock.mockResolvedValue({ ok: false, status: 401, json: async () => ({}) });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("expired-credentials");
  });

  it("두 창이 모두 비어 있으면 응답 형식이 바뀐 것으로 보고 error를 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    fetchMock.mockResolvedValue({ ok: true, status: 200, json: async () => ({}) });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("error");
    expect(result.reason).toBe("empty-response");
  });
});
