import { describe, expect, it } from "vitest";
import { fromCachedSnapshot, toCacheableSnapshot } from "@/lib/aiUsage/usageSnapshotCache";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

function createSnapshot(): AiUsageSnapshot {
  return {
    fetchedAt: "2026-08-10T06:00:00.000Z",
    accounts: [
      {
        provider: "claude",
        accountId: "e1c9e6ac-0000-4000-8000-000000000000",
        label: "me@example.com",
        status: "ok",
        planName: "max",
        windows: [{ kind: "session", modelName: null, usedPercent: 22, resetsAt: null }],
        reason: null,
        fetchedAt: "2026-08-10T06:00:00.000Z",
      },
    ],
  };
}

describe("toCacheableSnapshot", () => {
  it("계정 식별자를 원본 그대로 저장하지 않는다", () => {
    const serialized = toCacheableSnapshot(createSnapshot());

    expect(serialized).not.toContain("e1c9e6ac-0000-4000-8000-000000000000");
  });

  it("같은 계정은 조회를 반복해도 같은 식별자로 저장한다", () => {
    const first = fromCachedSnapshot(toCacheableSnapshot(createSnapshot()));
    const second = fromCachedSnapshot(toCacheableSnapshot(createSnapshot()));

    expect(first?.accounts[0].accountId).toBe(second?.accounts[0].accountId);
  });

  it("화면이 그리는 값은 그대로 살려 둔다", () => {
    const restored = fromCachedSnapshot(toCacheableSnapshot(createSnapshot()));

    expect(restored?.fetchedAt).toBe("2026-08-10T06:00:00.000Z");
    expect(restored?.accounts[0]).toMatchObject({
      provider: "claude",
      label: "me@example.com",
      status: "ok",
      planName: "max",
      reason: null,
      windows: [{ kind: "session", modelName: null, usedPercent: 22, resetsAt: null }],
    });
  });

  it("스냅샷에 없던 필드는 저장하지 않는다", () => {
    const snapshot = createSnapshot();
    (snapshot.accounts[0] as unknown as { accessToken: string }).accessToken = "secret-token";

    expect(toCacheableSnapshot(snapshot)).not.toContain("secret-token");
  });
});

describe("fromCachedSnapshot", () => {
  it("저장된 값이 없으면 null을 돌려준다", () => {
    expect(fromCachedSnapshot(null)).toBeNull();
    expect(fromCachedSnapshot("")).toBeNull();
  });

  it("깨진 JSON은 캐시를 버린다", () => {
    expect(fromCachedSnapshot("{not json")).toBeNull();
  });

  it("스키마 버전이 다르면 캐시를 버린다", () => {
    const stored = JSON.parse(toCacheableSnapshot(createSnapshot())) as { version: number };
    stored.version = 99;

    expect(fromCachedSnapshot(JSON.stringify(stored))).toBeNull();
  });

  it("모르는 provider나 status가 섞여 있으면 캐시를 버린다", () => {
    const stored = JSON.parse(toCacheableSnapshot(createSnapshot())) as {
      snapshot: { accounts: { provider: string }[] };
    };
    stored.snapshot.accounts[0].provider = "grok";

    expect(fromCachedSnapshot(JSON.stringify(stored))).toBeNull();
  });
});
