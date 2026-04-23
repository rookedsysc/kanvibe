import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockExecGit, mockGetHookServerUrl, mockGetHookServerToken, mockFetch } = vi.hoisted(() => ({
  mockExecGit: vi.fn(),
  mockGetHookServerUrl: vi.fn(),
  mockGetHookServerToken: vi.fn(),
  mockFetch: vi.fn(),
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
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

  it("treats a local health check failure as not installed", async () => {
    mockFetch.mockRejectedValueOnce(new Error("connection refused"));
    const { validateHookServerConfiguration } = await import("@/lib/hookServerStatus");

    const result = await validateHookServerConfiguration(["http://localhost:9736"], true, null);

    expect(result.hasExpectedHookServerUrl).toBe(true);
    expect(result.hasReachableHookServer).toBe(false);
  });
});
