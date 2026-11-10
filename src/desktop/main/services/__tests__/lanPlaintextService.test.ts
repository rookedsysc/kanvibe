// @vitest-environment node
import { beforeEach, describe, expect, it, vi } from "vitest";

const settings = vi.hoisted(() => ({
  isAllowed: false,
  observedSince: 0,
  lastRequestAt: 0,
  shouldFail: false,
  recorded: [] as number[],
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getLanPlaintextAllowed: async () => {
    if (settings.shouldFail) throw new Error("db down");
    return settings.isAllowed;
  },
  getOrStartLanPlaintextObservation: async (now: number) => {
    settings.observedSince ||= now;
    return settings.observedSince;
  },
  getLanPlaintextLastRequestAt: async () => settings.lastRequestAt,
  setLanPlaintextLastRequestAt: async (requestedAt: number) => {
    settings.recorded.push(requestedAt);
  },
}));

const DAY_MS = 24 * 60 * 60_000;
const NOW = 100 * DAY_MS;

describe("lanPlaintextService", () => {
  beforeEach(() => {
    Object.assign(settings, { isAllowed: false, observedSince: 0, lastRequestAt: 0, shouldFail: false, recorded: [] });
    vi.resetModules();
  });

  async function loadService() {
    return import("@/desktop/main/services/lanPlaintextService");
  }

  it("처음 실행하면 사용량을 재는 동안 LAN에 연다", async () => {
    const { resolveHookServerBindHost } = await loadService();
    expect(await resolveHookServerBindHost(NOW)).toBe("0.0.0.0");
    expect(settings.observedSince).toBe(NOW);
  });

  it("14일 동안 LAN 평문 요청이 없으면 루프백으로 닫는다", async () => {
    settings.observedSince = NOW - 14 * DAY_MS;
    const { resolveHookServerBindHost } = await loadService();
    expect(await resolveHookServerBindHost(NOW)).toBe("127.0.0.1");
  });

  it("14일 안에 LAN 평문 요청이 있었으면 열어 둔다", async () => {
    settings.observedSince = NOW - 30 * DAY_MS;
    settings.lastRequestAt = NOW - 13 * DAY_MS;
    const { resolveHookServerBindHost } = await loadService();
    expect(await resolveHookServerBindHost(NOW)).toBe("0.0.0.0");
  });

  it("사용자가 허용하면 사용량과 무관하게 연다", async () => {
    settings.observedSince = NOW - 30 * DAY_MS;
    settings.isAllowed = true;
    const { resolveHookServerBindHost } = await loadService();
    expect(await resolveHookServerBindHost(NOW)).toBe("0.0.0.0");
  });

  it("설정을 읽지 못하면 닫을 근거가 없어 기존처럼 연다", async () => {
    settings.shouldFail = true;
    const { resolveHookServerBindHost } = await loadService();
    expect(await resolveHookServerBindHost(NOW)).toBe("0.0.0.0");
  });

  it.each(["127.0.0.1", "::1", "::ffff:127.0.0.1"])("루프백 요청 %s은 LAN 사용량으로 세지 않는다", async (address) => {
    const { recordLanPlaintextResponse } = await loadService();
    recordLanPlaintextResponse(address, "/api/hooks/status", 200, NOW);
    expect(settings.recorded).toEqual([]);
  });

  it("LAN 요청은 기록하되 한 시간에 한 번만 쓴다", async () => {
    const { recordLanPlaintextResponse } = await loadService();
    recordLanPlaintextResponse("192.168.0.20", "/api/hooks/status", 200, NOW);
    recordLanPlaintextResponse("192.168.0.20", "/api/hooks/status", 200, NOW + 1000);
    recordLanPlaintextResponse("192.168.0.20", "/api/hooks/status", 200, NOW + 60 * 60_000);
    expect(settings.recorded).toEqual([NOW, NOW + 60 * 60_000]);
  });

  it.each([
    ["없는 경로", "/wp-login.php", 404],
    ["인증 실패", "/api/mobile/board", 401],
    ["인증 없이 성공하는 상태 확인", "/api/hooks/health", 200],
  ])("%s 요청은 LAN 사용량으로 세지 않아 스캐너가 평문 포트를 열어 둘 수 없다", async (_name, requestUrl, statusCode) => {
    const { recordLanPlaintextResponse } = await loadService();
    recordLanPlaintextResponse("192.168.0.20", requestUrl, statusCode, NOW);
    expect(settings.recorded).toEqual([]);
  });
});
