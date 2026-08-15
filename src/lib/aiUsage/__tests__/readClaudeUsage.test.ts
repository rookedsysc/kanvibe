import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClaudeUsage } from "@/lib/aiUsage/readClaudeUsage";
import type { AiUsageAccount } from "@/lib/aiUsage/types";

const {
  mockReadTextFile,
  mockIsClaudeTokenExpiring,
  mockRefreshClaudeCredentials,
  mockWriteCredentialsAtomically,
  mockReadClaudeKeychainCredentials,
} = vi.hoisted(() => ({
  mockReadTextFile: vi.fn(),
  mockIsClaudeTokenExpiring: vi.fn(),
  mockRefreshClaudeCredentials: vi.fn(),
  mockWriteCredentialsAtomically: vi.fn(),
  mockReadClaudeKeychainCredentials: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({ readTextFile: mockReadTextFile }));

vi.mock("@/lib/aiUsage/claudeCredentials", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/aiUsage/claudeCredentials")>()),
  readClaudeKeychainCredentials: mockReadClaudeKeychainCredentials,
}));

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
    mockReadClaudeKeychainCredentials.mockResolvedValue({ outcome: "absent" });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  /** 429 대기 시각은 모듈이 기억하므로, 그 동작을 보는 테스트는 새 인스턴스를 받아야 서로 오염되지 않는다 */
  async function loadFreshReadClaudeUsage() {
    vi.resetModules();
    return (await import("@/lib/aiUsage/readClaudeUsage")).readClaudeUsage;
  }

  function stubRateLimitedResponse(retryAfterSeconds: string | null): void {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 429,
      headers: { get: () => retryAfterSeconds },
      json: async () => ({}),
    });
  }

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

  it("자격증명 파일도 Keychain도 비어 있으면 네트워크를 호출하지 않고 unavailable을 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue("");

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("missing-credentials");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("자격증명 파일이 없으면 Keychain의 토큰으로 조회한다", async () => {
    mockReadTextFile.mockResolvedValue("");
    mockReadClaudeKeychainCredentials.mockResolvedValue({
      outcome: "found",
      credentials: createCredentialsJson("keychain-token"),
    });
    stubUsageResponse();

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("ok");
    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers.Authorization).toBe("Bearer keychain-token");
  });

  it("Keychain에서 온 자격증명은 만료가 임박해도 갱신하거나 되쓰지 않는다", async () => {
    mockReadTextFile.mockResolvedValue("");
    mockReadClaudeKeychainCredentials.mockResolvedValue({
      outcome: "found",
      credentials: createCredentialsJson("keychain-token"),
    });
    mockIsClaudeTokenExpiring.mockReturnValue(true);
    stubUsageResponse();

    await readClaudeUsage(ACCOUNT);

    expect(mockRefreshClaudeCredentials).not.toHaveBeenCalled();
    expect(mockWriteCredentialsAtomically).not.toHaveBeenCalled();
  });

  it("Keychain을 읽지 못하면 로그인하지 않은 상태로 오해시키지 않는다", async () => {
    mockReadTextFile.mockResolvedValue("");
    mockReadClaudeKeychainCredentials.mockResolvedValue({ outcome: "unreadable" });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("keychain-unreadable");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("401 응답은 재로그인이 필요한 부재로 분류한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("expired-token"));
    fetchMock.mockResolvedValue({ ok: false, status: 401, json: async () => ({}) });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("expired-credentials");
  });

  it("429를 받으면 retry-after가 지나기 전에는 다시 호출하지 않는다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    const readFreshClaudeUsage = await loadFreshReadClaudeUsage();
    stubRateLimitedResponse("290");

    const first = await readFreshClaudeUsage(ACCOUNT);
    const second = await readFreshClaudeUsage(ACCOUNT);

    expect(first.reason).toBe("rate-limited");
    expect(second.reason).toBe("rate-limited");
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("retry-after가 지나면 다시 호출한다", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-15T10:00:00.000Z"));
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    const readFreshClaudeUsage = await loadFreshReadClaudeUsage();
    stubRateLimitedResponse("60");

    await readFreshClaudeUsage(ACCOUNT);
    vi.setSystemTime(new Date("2026-08-15T10:01:01.000Z"));
    stubUsageResponse();
    const result = await readFreshClaudeUsage(ACCOUNT);

    expect(result.status).toBe("ok");
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("retry-after를 주지 않아도 한동안은 다시 호출하지 않는다", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-15T10:00:00.000Z"));
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    const readFreshClaudeUsage = await loadFreshReadClaudeUsage();
    stubRateLimitedResponse(null);

    await readFreshClaudeUsage(ACCOUNT);
    vi.setSystemTime(new Date("2026-08-15T10:04:59.000Z"));
    await readFreshClaudeUsage(ACCOUNT);

    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("모델별 주간 한도는 7일 창과 같은 종류로 묶고 모델 이름으로 가른다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        five_hour: { utilization: 9, resets_at: "2026-08-15T15:09:59.000Z" },
        seven_day: { utilization: 1, resets_at: "2026-08-16T17:59:59.000Z" },
        limits: [
          { kind: "session", percent: 9, resets_at: "2026-08-15T15:09:59.000Z", scope: null },
          { kind: "weekly_all", percent: 1, resets_at: "2026-08-16T17:59:59.000Z", scope: null },
          {
            kind: "weekly_scoped",
            percent: 4,
            resets_at: null,
            scope: { model: { id: null, display_name: "Fable" } },
          },
        ],
      }),
    });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.windows).toEqual([
      { kind: "session", modelName: null, usedPercent: 9, resetsAt: "2026-08-15T15:09:59.000Z" },
      { kind: "weekly", modelName: null, usedPercent: 1, resetsAt: "2026-08-16T17:59:59.000Z" },
      { kind: "weekly", modelName: "Fable", usedPercent: 4, resetsAt: null },
    ]);
  });

  it("이름 없는 모델 창과 모르는 창 이름은 버린다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        limits: [
          { kind: "session", percent: 9, resets_at: null, scope: null },
          { kind: "weekly_scoped", percent: 4, resets_at: null, scope: { model: null } },
          { kind: "nimbus_quill", percent: 7, resets_at: null, scope: null },
        ],
      }),
    });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.windows).toEqual([
      { kind: "session", modelName: null, usedPercent: 9, resetsAt: null },
    ]);
  });

  it("자격증명의 구독 등급을 플랜 이름으로 쓴다", async () => {
    mockReadTextFile.mockResolvedValue(
      JSON.stringify({ claudeAiOauth: { accessToken: "access-token", subscriptionType: "max" } }),
    );
    stubUsageResponse();

    expect((await readClaudeUsage(ACCOUNT)).planName).toBe("max");
  });

  it("두 창이 모두 비어 있으면 응답 형식이 바뀐 것으로 보고 error를 돌려준다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("access-token"));
    fetchMock.mockResolvedValue({ ok: true, status: 200, json: async () => ({}) });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("error");
    expect(result.reason).toBe("empty-response");
  });
});
