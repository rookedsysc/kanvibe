import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const socket = {
    once: vi.fn(),
    connect: vi.fn(),
    address: vi.fn(() => ({ address: "10.0.0.8", family: "IPv4", port: 2202 })),
    close: vi.fn(),
  };

  socket.once.mockImplementation(() => socket);
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
  beforeEach(() => {
    vi.resetModules();
    delete globalThis.__KANVIBE_HOOK_SERVER_PORT__;
    mocks.networkInterfaces.mockReset();
    mocks.parseSSHConfig.mockReset();
    mocks.lookup.mockReset();
    mocks.createSocket.mockClear();
    mocks.socket.address.mockReturnValue({ address: "10.0.0.8", family: "IPv4", port: 2202 });
  });

  it("desktop main이 설정한 hook server port를 hook URL에 사용한다", async () => {
    // Given
    const { getHookServerUrl, setHookServerPort } = await import("@/lib/hookEndpoint");
    setHookServerPort(19736);

    // When
    const result = await getHookServerUrl(null);

    // Then
    expect(result).toBe("http://localhost:19736");
  });

  it("desktop dev hook server port 상수는 pnpm dev 포트와 일치한다", async () => {
    // Given
    const { KANVIBE_DEV_HOOK_SERVER_PORT } = await import("@/lib/hookEndpoint");

    // Then
    expect(KANVIBE_DEV_HOOK_SERVER_PORT).toBe(19736);
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

  it("설정된 hook server port를 원격 hook 주소에도 반영한다", async () => {
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
    const { getHookServerUrl, setHookServerPort } = await import("@/lib/hookEndpoint");
    setHookServerPort(19736);

    // When
    const result = await getHookServerUrl("remote-devbox");

    // Then
    expect(result).toBe("http://10.0.0.8:19736");
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
});
