import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mockNetworkInterfaces = vi.fn();

vi.mock("node:os", () => ({
  default: {
    networkInterfaces: mockNetworkInterfaces,
  },
  networkInterfaces: mockNetworkInterfaces,
}));

describe("hookEndpoint", () => {
  const originalExternalHost = process.env.KANVIBE_EXTERNAL_HOST;
  const originalHookToken = process.env.KANVIBE_HOOK_TOKEN;

  beforeEach(() => {
    vi.resetModules();
    delete process.env.KANVIBE_EXTERNAL_HOST;
    delete process.env.KANVIBE_HOOK_TOKEN;
    mockNetworkInterfaces.mockReset();
  });

  afterEach(() => {
    if (originalExternalHost === undefined) {
      delete process.env.KANVIBE_EXTERNAL_HOST;
    } else {
      process.env.KANVIBE_EXTERNAL_HOST = originalExternalHost;
    }

    if (originalHookToken === undefined) {
      delete process.env.KANVIBE_HOOK_TOKEN;
    } else {
      process.env.KANVIBE_HOOK_TOKEN = originalHookToken;
    }
  });

  it("원격 호스트가 있으면 명시된 외부 주소를 hook 서버 경로로 사용한다", async () => {
    // Given
    process.env.KANVIBE_EXTERNAL_HOST = "192.168.0.5";
    const { getHookServerUrl } = await import("@/lib/hookEndpoint");

    // When
    const result = getHookServerUrl("remote-devbox");

    // Then
    expect(result).toBe("http://192.168.0.5:9736");
  });

  it("원격 호스트가 없으면 내부망 IPv4 주소를 우선 선택한다", async () => {
    // Given
    mockNetworkInterfaces.mockReturnValue({
      lo: [{ address: "127.0.0.1", family: "IPv4", internal: true }],
      wlan0: [{ address: "192.168.1.20", family: "IPv4", internal: false }],
      eth0: [{ address: "8.8.8.8", family: "IPv4", internal: false }],
    });
    const { getHookServerUrl } = await import("@/lib/hookEndpoint");

    // When
    const result = getHookServerUrl("remote-devbox");

    // Then
    expect(result).toBe("http://192.168.1.20:9736");
  });

  it("원격 호스트가 없으면 localhost 경로를 사용한다", async () => {
    // Given
    const { getHookServerUrl } = await import("@/lib/hookEndpoint");

    // When
    const result = getHookServerUrl(null);

    // Then
    expect(result).toBe("http://localhost:9736");
  });

  it("hook 토큰은 환경변수 값을 그대로 반환한다", async () => {
    // Given
    process.env.KANVIBE_HOOK_TOKEN = "secret-token";
    const { getHookServerToken } = await import("@/lib/hookEndpoint");

    // When
    const result = getHookServerToken();

    // Then
    expect(result).toBe("secret-token");
  });
});
