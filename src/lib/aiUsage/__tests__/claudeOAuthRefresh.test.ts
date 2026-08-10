import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  applyRefreshedClaudeToken,
  isClaudeTokenExpiring,
  refreshClaudeCredentials,
} from "@/lib/aiUsage/claudeOAuthRefresh";

const ONE_MINUTE_MS = 60 * 1000;
const NOW = Date.parse("2026-08-10T08:00:00.000Z");

function createCredentialsJson(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    mcpOAuth: { "notion|abc": { token: "keep-me" } },
    claudeAiOauth: {
      accessToken: "old-access-token",
      refreshToken: "old-refresh-token",
      expiresAt: NOW + 60 * ONE_MINUTE_MS,
      scopes: ["user:inference"],
      subscriptionType: "max",
      ...overrides,
    },
  });
}

describe("isClaudeTokenExpiring", () => {
  it("만료 5분 버퍼 안에 들어오면 갱신 대상으로 본다", () => {
    expect(isClaudeTokenExpiring(createCredentialsJson({ expiresAt: NOW + 4 * ONE_MINUTE_MS }), NOW)).toBe(true);
  });

  it("버퍼 밖이면 갱신하지 않는다", () => {
    expect(isClaudeTokenExpiring(createCredentialsJson({ expiresAt: NOW + 10 * ONE_MINUTE_MS }), NOW)).toBe(false);
  });

  it("만료 시각을 알 수 없으면 신뢰하지 않고 갱신 대상으로 본다", () => {
    expect(isClaudeTokenExpiring(createCredentialsJson({ expiresAt: undefined }), NOW)).toBe(true);
  });

  it("OAuth 블록이 아예 없으면 갱신할 대상이 없다", () => {
    expect(isClaudeTokenExpiring(JSON.stringify({ mcpOAuth: {} }), NOW)).toBe(false);
  });
});

describe("applyRefreshedClaudeToken", () => {
  it("회전된 refresh 토큰을 저장하고 다른 최상위 키를 잃지 않는다", () => {
    const merged = applyRefreshedClaudeToken(
      createCredentialsJson(),
      { access_token: "new-access-token", expires_in: 3600, refresh_token: "rotated-refresh-token" },
      NOW,
    );

    const parsed = JSON.parse(merged ?? "{}");
    expect(parsed.claudeAiOauth.accessToken).toBe("new-access-token");
    expect(parsed.claudeAiOauth.refreshToken).toBe("rotated-refresh-token");
    expect(parsed.claudeAiOauth.expiresAt).toBe(NOW + 3600 * 1000);
    expect(parsed.claudeAiOauth.subscriptionType).toBe("max");
    expect(parsed.mcpOAuth).toEqual({ "notion|abc": { token: "keep-me" } });
  });

  it("응답이 refresh 토큰을 주지 않으면 기존 값을 보존한다", () => {
    const merged = applyRefreshedClaudeToken(
      createCredentialsJson(),
      { access_token: "new-access-token", expires_in: 3600 },
      NOW,
    );

    expect(JSON.parse(merged ?? "{}").claudeAiOauth.refreshToken).toBe("old-refresh-token");
  });

  it("액세스 토큰이 없는 응답은 병합하지 않는다", () => {
    expect(applyRefreshedClaudeToken(createCredentialsJson(), { expires_in: 3600 }, NOW)).toBeNull();
  });
});

describe("refreshClaudeCredentials", () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("공개 Claude Code client id로 refresh_token 교환을 요청한다", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({ access_token: "new-access-token", expires_in: 3600 }),
    });

    const refreshed = await refreshClaudeCredentials(createCredentialsJson(), NOW);

    const [requestUrl, requestInit] = fetchMock.mock.calls[0];
    expect(requestUrl).toBe("https://platform.claude.com/v1/oauth/token");
    const body = new URLSearchParams(requestInit.body as string);
    expect(body.get("grant_type")).toBe("refresh_token");
    expect(body.get("refresh_token")).toBe("old-refresh-token");
    expect(body.get("client_id")).toBe("9d1c250a-e61b-44d9-88ed-5944d1962f5e");
    expect(JSON.parse(refreshed ?? "{}").claudeAiOauth.accessToken).toBe("new-access-token");
  });

  /** 갱신 실패가 로그인 파괴로 번지면 안 되므로 호출부가 기존 자격증명을 그대로 쓰도록 null을 돌려준다 */
  it("토큰 엔드포인트가 429를 주면 병합하지 않고 null을 돌려준다", async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 429, json: async () => ({}) });

    await expect(refreshClaudeCredentials(createCredentialsJson(), NOW)).resolves.toBeNull();
  });

  it("네트워크 오류도 null로 흡수한다", async () => {
    fetchMock.mockRejectedValue(new Error("network down"));

    await expect(refreshClaudeCredentials(createCredentialsJson(), NOW)).resolves.toBeNull();
  });

  it("refresh 토큰이 없으면 네트워크를 호출하지 않는다", async () => {
    await expect(
      refreshClaudeCredentials(createCredentialsJson({ refreshToken: "" }), NOW),
    ).resolves.toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
