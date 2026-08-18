import { afterEach, describe, expect, it, vi } from "vitest";
import { aggregateAiUsage } from "@/lib/aiUsage/aggregateAiUsage";
import type { AiUsageAccount, AiUsageAccountResult, AiUsageProvider } from "@/lib/aiUsage/types";

const {
  mockDiscoverClaudeAccounts,
  mockDiscoverCodexAccounts,
  mockDiscoverGeminiAccounts,
  mockReadClaudeUsage,
  mockReadCodexUsage,
  mockReadGeminiUsage,
} = vi.hoisted(() => ({
  mockDiscoverClaudeAccounts: vi.fn(),
  mockDiscoverCodexAccounts: vi.fn(),
  mockDiscoverGeminiAccounts: vi.fn(),
  mockReadClaudeUsage: vi.fn(),
  mockReadCodexUsage: vi.fn(),
  mockReadGeminiUsage: vi.fn(),
}));

vi.mock("@/lib/aiUsage/accountDiscovery", () => ({
  discoverProviderAccounts: (provider: AiUsageProvider) => ({
    claude: mockDiscoverClaudeAccounts,
    codex: mockDiscoverCodexAccounts,
    gemini: mockDiscoverGeminiAccounts,
  })[provider](),
}));

vi.mock("@/lib/aiUsage/readClaudeUsage", () => ({ readClaudeUsage: mockReadClaudeUsage }));
vi.mock("@/lib/aiUsage/readCodexUsage", () => ({ readCodexUsage: mockReadCodexUsage }));
vi.mock("@/lib/aiUsage/readGeminiUsage", () => ({ readGeminiUsage: mockReadGeminiUsage }));

function createAccount(provider: AiUsageProvider, accountId: string): AiUsageAccount {
  return {
    provider,
    accountId,
    label: `${accountId}@example.com`,
    configDir: `/home/tester/${accountId}`,
    accountRoot: `/home/tester/${accountId}`,
  };
}

function createOkResult(account: AiUsageAccount): AiUsageAccountResult {
  return {
    provider: account.provider,
    accountId: account.accountId,
    label: account.label,
    status: "ok",
    planName: null,
    windows: [{ kind: "weekly", modelName: null, usedPercent: 10, resetsAt: null }],
    reason: null,
    fetchedAt: "2026-08-10T00:00:00.000Z",
  };
}

function discoverNothing(): void {
  mockDiscoverClaudeAccounts.mockResolvedValue([]);
  mockDiscoverCodexAccounts.mockResolvedValue([]);
  mockDiscoverGeminiAccounts.mockResolvedValue([]);
}

describe("aggregateAiUsage", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("계정을 하나도 못 찾은 provider도 카드 자리를 남긴다", async () => {
    discoverNothing();

    const snapshot = await aggregateAiUsage();

    expect(snapshot.accounts.map((result) => result.provider)).toEqual(["claude", "codex", "gemini"]);
    expect(snapshot.accounts.every((result) => result.status === "unavailable")).toBe(true);
    expect(snapshot.accounts.every((result) => result.reason === "missing-credentials")).toBe(true);
    expect(mockReadClaudeUsage).not.toHaveBeenCalled();
  });

  it("한 provider에 계정이 여럿이면 계정마다 결과를 만든다", async () => {
    discoverNothing();
    const personal = createAccount("claude", "personal");
    const work = createAccount("claude", "work");
    mockDiscoverClaudeAccounts.mockResolvedValue([personal, work]);
    mockReadClaudeUsage.mockImplementation(async (account: AiUsageAccount) => createOkResult(account));

    const snapshot = await aggregateAiUsage();

    const claudeResults = snapshot.accounts.filter((result) => result.provider === "claude");
    expect(claudeResults.map((result) => result.accountId)).toEqual(["personal", "work"]);
    expect(mockReadClaudeUsage).toHaveBeenCalledWith(personal);
    expect(mockReadClaudeUsage).toHaveBeenCalledWith(work);
  });

  it("한 계정 조회가 예외를 던져도 나머지 결과를 잃지 않는다", async () => {
    discoverNothing();
    const claudeAccount = createAccount("claude", "personal");
    const codexAccount = createAccount("codex", "codex-seat");
    mockDiscoverClaudeAccounts.mockResolvedValue([claudeAccount]);
    mockDiscoverCodexAccounts.mockResolvedValue([codexAccount]);
    mockReadClaudeUsage.mockRejectedValue(new Error("network down"));
    mockReadCodexUsage.mockResolvedValue(createOkResult(codexAccount));

    const snapshot = await aggregateAiUsage();

    const claudeResult = snapshot.accounts.find((result) => result.provider === "claude");
    expect(claudeResult?.status).toBe("error");
    expect(claudeResult?.reason).toBe("fetch-failed");
    expect(claudeResult?.label).toBe(claudeAccount.label);
    expect(snapshot.accounts.find((result) => result.provider === "codex")?.status).toBe("ok");
  });

  it("계정 발견이 실패한 provider는 빈 자리로 두고 나머지를 조회한다", async () => {
    discoverNothing();
    const codexAccount = createAccount("codex", "codex-seat");
    mockDiscoverClaudeAccounts.mockRejectedValue(new Error("home unreadable"));
    mockDiscoverCodexAccounts.mockResolvedValue([codexAccount]);
    mockReadCodexUsage.mockResolvedValue(createOkResult(codexAccount));

    const snapshot = await aggregateAiUsage();

    expect(snapshot.accounts.find((result) => result.provider === "claude")?.status).toBe("unavailable");
    expect(snapshot.accounts.find((result) => result.provider === "codex")?.status).toBe("ok");
    expect(snapshot.fetchedAt).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });
});
