import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockExecGit, mockGetHookServerUrl, mockGetHookServerToken, mockFetch, mockIsSSHTransportError } = vi.hoisted(() => ({
  mockExecGit: vi.fn(),
  mockGetHookServerUrl: vi.fn(),
  mockGetHookServerToken: vi.fn(),
  mockFetch: vi.fn(),
  mockIsSSHTransportError: vi.fn((error: unknown) => /Connection reset/i.test(String(error))),
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
  isSSHTransportError: (error: unknown) => mockIsSSHTransportError(error),
}));

vi.mock("@/lib/hookEndpoint", () => ({
  getHookServerUrl: (...args: unknown[]) => mockGetHookServerUrl(...args),
  getHookServerToken: (...args: unknown[]) => mockGetHookServerToken(...args),
}));

describe("hookServerStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetHookServerUrl.mockResolvedValue("http://localhost:9736");
    mockGetHookServerToken.mockReturnValue("desktop-hook-token");
    mockExecGit.mockResolvedValue("");
    mockFetch.mockResolvedValue({ ok: true });
    mockIsSSHTransportError.mockImplementation((error: unknown) => /Connection reset/i.test(String(error)));
    vi.stubGlobal("fetch", mockFetch);
  });

  it("marks stale configured hook URLs as invalid", async () => {
    const { validateHookServerConfiguration } = await import("@/lib/hookServerStatus");

    const result = await validateHookServerConfiguration(["http://192.168.0.8:9736"], true, "remote-host");

    expect(result.hasExpectedHookServerUrl).toBe(false);
    expect(result.hasReachableHookServer).toBe(false);
    expect(mockExecGit).not.toHaveBeenCalled();
  });

  it("checks hook server reachability from the remote host", async () => {
    mockGetHookServerUrl.mockResolvedValue("http://10.0.0.4:9736");
    const { validateHookServerConfiguration } = await import("@/lib/hookServerStatus");

    const result = await validateHookServerConfiguration(["http://10.0.0.4:9736"], true, "remote-host");

    expect(result.hasExpectedHookServerUrl).toBe(true);
    expect(result.hasReachableHookServer).toBe(true);
    expect(mockExecGit).toHaveBeenCalledWith(
      expect.stringContaining("curl -fsS --max-time 2"),
      "remote-host",
    );
  });

  it("treats remote SSH transport failures as inconclusive instead of uninstalling hooks", async () => {
    mockGetHookServerUrl.mockResolvedValue("http://10.0.0.4:9736");
    mockExecGit.mockRejectedValueOnce(new Error("remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22"));
    const { validateHookServerConfiguration } = await import("@/lib/hookServerStatus");

    const result = await validateHookServerConfiguration(["http://10.0.0.4:9736"], true, "remote-host");

    expect(result.hasExpectedHookServerUrl).toBe(true);
    expect(result.hasReachableHookServer).toBe(true);
  });

  it("treats a local health check failure as not installed", async () => {
    mockFetch.mockRejectedValueOnce(new Error("connection refused"));
    const { validateHookServerConfiguration } = await import("@/lib/hookServerStatus");

    const result = await validateHookServerConfiguration(["http://localhost:9736"], true, null);

    expect(result.hasExpectedHookServerUrl).toBe(true);
    expect(result.hasReachableHookServer).toBe(false);
  });
});
