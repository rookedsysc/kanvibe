import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClaudeUsage } from "@/lib/aiUsage/readClaudeUsage";
import type { AiUsageAccount } from "@/lib/aiUsage/types";

const { mockReadTextFile, mockReadClaudeKeychainCredentials } = vi.hoisted(() => ({
  mockReadTextFile: vi.fn(),
  mockReadClaudeKeychainCredentials: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({ readTextFile: mockReadTextFile }));

vi.mock("@/lib/aiUsage/claudeCredentials", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/aiUsage/claudeCredentials")>()),
  readClaudeKeychainCredentials: mockReadClaudeKeychainCredentials,
}));

const ACCOUNT: AiUsageAccount = {
  provider: "claude",
  accountId: "account-uuid",
  label: "me@example.com",
  configDir: "/home/tester/.claude",
  accountRoot: "/home/tester/.claude",
};

const CLAUDE_USAGE_URL = "https://api.anthropic.com/api/oauth/usage";

/** 테스트가 도는 동안 지나지 않는 만료 시각과 이미 지난 만료 시각 */
const UNEXPIRED_AT_MS = Date.parse("2999-01-01T00:00:00.000Z");
const EXPIRED_AT_MS = 1_000;

function createCredentialsJson(accessToken: string, expiresAt?: number): string {
  return JSON.stringify({
    claudeAiOauth: { accessToken, refreshToken: "refresh-token", expiresAt },
  });
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

  it("액세스 토큰이 만료됐고 refresh 토큰이 살아 있으면 조회하지 않고 Claude 실행을 기다린다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("expired-token", EXPIRED_AT_MS));

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.reason).toBe("awaiting-cli-refresh");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("액세스 토큰이 만료됐는데 refresh 토큰이 없으면 재로그인이 필요하다", async () => {
    mockReadTextFile.mockResolvedValue(
      JSON.stringify({ claudeAiOauth: { accessToken: "expired-token", expiresAt: EXPIRED_AT_MS } }),
    );

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.reason).toBe("expired-credentials");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("액세스 토큰과 refresh 토큰이 모두 만료됐으면 재로그인이 필요하다", async () => {
    mockReadTextFile.mockResolvedValue(
      JSON.stringify({
        claudeAiOauth: {
          accessToken: "expired-token",
          refreshToken: "expired-refresh-token",
          expiresAt: EXPIRED_AT_MS,
          refreshTokenExpiresAt: EXPIRED_AT_MS,
        },
      }),
    );

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.reason).toBe("expired-credentials");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("만료 시각을 모르는 토큰은 만료됐다고 단정하지 않고 조회한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("token-without-expiry"));
    stubUsageResponse();

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("ok");
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("만료가 임박한 토큰도 갱신하지 않고 저장된 값 그대로 사용량만 조회한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("soon-expiring-token", Date.now() + 60_000));
    stubUsageResponse();

    await readClaudeUsage(ACCOUNT);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [requestUrl, requestInit] = fetchMock.mock.calls[0];
    expect(requestUrl).toBe(CLAUDE_USAGE_URL);
    expect(requestInit.headers.Authorization).toBe("Bearer soon-expiring-token");
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

  it("자격증명 파일이 Keychain보다 오래됐으면 Keychain 토큰으로 조회한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("stale-file-token", UNEXPIRED_AT_MS));
    mockReadClaudeKeychainCredentials.mockResolvedValue({
      outcome: "found",
      credentials: createCredentialsJson("fresh-keychain-token", UNEXPIRED_AT_MS + 1_000),
    });
    stubUsageResponse();

    await readClaudeUsage(ACCOUNT);

    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers.Authorization).toBe("Bearer fresh-keychain-token");
  });

  it("자격증명 파일이 Keychain보다 새로우면 파일 토큰으로 조회한다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("fresh-file-token", UNEXPIRED_AT_MS + 1_000));
    mockReadClaudeKeychainCredentials.mockResolvedValue({
      outcome: "found",
      credentials: createCredentialsJson("stale-keychain-token", UNEXPIRED_AT_MS),
    });
    stubUsageResponse();

    await readClaudeUsage(ACCOUNT);

    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers.Authorization).toBe("Bearer fresh-file-token");
  });

  it("만료 시각을 아는 쪽이 한쪽뿐이면 그쪽을 쓴다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("file-token-without-expiry"));
    mockReadClaudeKeychainCredentials.mockResolvedValue({
      outcome: "found",
      credentials: createCredentialsJson("keychain-token", UNEXPIRED_AT_MS),
    });
    stubUsageResponse();

    await readClaudeUsage(ACCOUNT);

    const [, requestInit] = fetchMock.mock.calls[0];
    expect(requestInit.headers.Authorization).toBe("Bearer keychain-token");
  });

  it("Keychain을 읽지 못하면 로그인하지 않은 상태로 오해시키지 않는다", async () => {
    mockReadTextFile.mockResolvedValue("");
    mockReadClaudeKeychainCredentials.mockResolvedValue({ outcome: "unreadable" });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("keychain-unreadable");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("서버가 토큰을 거절해도 refresh 토큰이 살아 있으면 Claude 실행을 기다린다", async () => {
    mockReadTextFile.mockResolvedValue(createCredentialsJson("rejected-token", UNEXPIRED_AT_MS));
    fetchMock.mockResolvedValue({ ok: false, status: 401, json: async () => ({}) });

    const result = await readClaudeUsage(ACCOUNT);

    expect(result.status).toBe("unavailable");
    expect(result.reason).toBe("awaiting-cli-refresh");
  });

  it("서버가 토큰을 거절했고 refresh 토큰도 없으면 재로그인이 필요한 부재로 분류한다", async () => {
    mockReadTextFile.mockResolvedValue(
      JSON.stringify({ claudeAiOauth: { accessToken: "rejected-token", expiresAt: UNEXPIRED_AT_MS } }),
    );
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
