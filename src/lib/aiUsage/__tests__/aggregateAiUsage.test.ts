import { afterEach, describe, expect, it, vi } from "vitest";
import { aggregateAiUsage } from "@/lib/aiUsage/aggregateAiUsage";
import type { AiUsageProvider, AiUsageProviderResult } from "@/lib/aiUsage/types";

const { mockReadClaudeUsage, mockReadCodexUsage, mockReadGeminiUsage } = vi.hoisted(() => ({
  mockReadClaudeUsage: vi.fn(),
  mockReadCodexUsage: vi.fn(),
  mockReadGeminiUsage: vi.fn(),
}));

vi.mock("@/lib/aiUsage/readClaudeUsage", () => ({ readClaudeUsage: mockReadClaudeUsage }));
vi.mock("@/lib/aiUsage/readCodexUsage", () => ({ readCodexUsage: mockReadCodexUsage }));
vi.mock("@/lib/aiUsage/readGeminiUsage", () => ({ readGeminiUsage: mockReadGeminiUsage }));

function createOkResult(provider: AiUsageProvider): AiUsageProviderResult {
  return {
    provider,
    status: "ok",
    planName: null,
    windows: [{ kind: "weekly", modelName: null, usedPercent: 10, resetsAt: null }],
    reason: null,
    fetchedAt: "2026-08-10T00:00:00.000Z",
  };
}

describe("aggregateAiUsage", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

  it("한 provider가 예외를 던져도 나머지 결과를 잃지 않는다", async () => {
    mockReadClaudeUsage.mockRejectedValue(new Error("network down"));
    mockReadCodexUsage.mockResolvedValue(createOkResult("codex"));
    mockReadGeminiUsage.mockResolvedValue(createOkResult("gemini"));

    const snapshot = await aggregateAiUsage();

    expect(snapshot.providers.map((result) => result.provider)).toEqual([
      "claude",
      "codex",
      "gemini",
    ]);
    expect(snapshot.providers[0].status).toBe("error");
    expect(snapshot.providers[0].reason).toBe("fetch-failed");
    expect(snapshot.providers[1].status).toBe("ok");
    expect(snapshot.providers[2].status).toBe("ok");
  });

  it("항상 claude, codex, gemini 순서로 결과를 돌려준다", async () => {
    mockReadClaudeUsage.mockResolvedValue(createOkResult("claude"));
    mockReadCodexUsage.mockResolvedValue(createOkResult("codex"));
    mockReadGeminiUsage.mockResolvedValue(createOkResult("gemini"));

    const snapshot = await aggregateAiUsage();

    expect(snapshot.providers).toHaveLength(3);
    expect(snapshot.fetchedAt).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });
});
