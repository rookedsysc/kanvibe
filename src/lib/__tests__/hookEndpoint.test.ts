import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const socket = {
    once: vi.fn(),
    connect: vi.fn(),
    address: vi.fn(() => ({ address: "10.0.0.8", family: "IPv4", port: 2202 })),
    close: vi.fn(),
  };

  socket.once.mockImplementation((_event: string, _handler: () => void) => socket);
  socket.connect.mockImplementation((_port: number, _host: string, callback: () => void) => {
    callback();
    return socket;
  });

  return {
    networkInterfaces: vi.fn(),
    parseSSHConfig: vi.fn(),
    lookup: vi.fn(),
    createSocket: vi.fn(() => socket),
    socket,
  };
});

vi.mock("node:os", () => ({
  default: {
    networkInterfaces: mocks.networkInterfaces,
  },
  networkInterfaces: mocks.networkInterfaces,
}));

vi.mock("node:dns/promises", () => ({
  default: {
    lookup: mocks.lookup,
  },
  lookup: mocks.lookup,
}));

vi.mock("node:dgram", () => ({
  default: {
    createSocket: mocks.createSocket,
  },
  createSocket: mocks.createSocket,
}));

vi.mock("@/lib/sshConfig", () => ({
  parseSSHConfig: mocks.parseSSHConfig,
}));

describe("hookEndpoint", () => {
  const originalExternalHost = process.env.KANVIBE_EXTERNAL_HOST;
  const originalHookToken = process.env.KANVIBE_HOOK_TOKEN;

  beforeEach(() => {
    vi.resetModules();
    delete process.env.KANVIBE_EXTERNAL_HOST;
    delete process.env.KANVIBE_HOOK_TOKEN;
    mocks.networkInterfaces.mockReset();
    mocks.parseSSHConfig.mockReset();
    mocks.lookup.mockReset();
    mocks.createSocket.mockClear();
    mocks.socket.address.mockReturnValue({ address: "10.0.0.8", family: "IPv4", port: 2202 });
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
    const result = await getHookServerUrl("remote-devbox");

    // Then
    expect(result).toBe("http://192.168.0.5:9736");
  });

  it("원격 hook 주소는 SSH 연결에 사용한 로컬 출발 IP를 우선 사용한다", async () => {
    // Given
    mocks.parseSSHConfig.mockResolvedValue([
      {
        host: "remote-devbox",
        hostname: "ssh.example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      },
    ]);
    mocks.lookup.mockResolvedValue({ address: "203.0.113.20", family: 4 });
    const { getHookServerUrl } = await import("@/lib/hookEndpoint");

    // When
    const result = await getHookServerUrl("remote-devbox");

    // Then
    expect(mocks.createSocket).toHaveBeenCalledWith("udp4");
    expect(mocks.socket.connect).toHaveBeenCalledWith(2202, "203.0.113.20", expect.any(Function));
    expect(result).toBe("http://10.0.0.8:9736");
  });

  it("원격 호스트 경로를 해석하지 못하면 내부망 IPv4 주소를 사용한다", async () => {
    // Given
    mocks.parseSSHConfig.mockResolvedValue([]);
    mocks.lookup.mockResolvedValue({ address: "203.0.113.20", family: 4 });
    mocks.networkInterfaces.mockReturnValue({
      lo: [{ address: "127.0.0.1", family: "IPv4", internal: true }],
      wlan0: [{ address: "192.168.1.20", family: "IPv4", internal: false }],
    });
    mocks.socket.address.mockReturnValue({ address: "", family: "IPv4", port: 22 });
    const { getHookServerUrl } = await import("@/lib/hookEndpoint");

    // When
    const result = await getHookServerUrl("remote-devbox");

    // Then
    expect(result).toBe("http://192.168.1.20:9736");
  });

  it("원격 호스트가 없으면 localhost 경로를 사용한다", async () => {
    // Given
    const { getHookServerUrl } = await import("@/lib/hookEndpoint");

    // When
    const result = await getHookServerUrl(null);

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
