// @vitest-environment node
import { describe, it, expect, vi, beforeEach } from "vitest";
import { HOOK_INSTALL_SUCCESS_MARKER } from "@/lib/hookInstallBundle";

const mockExecGit = vi.fn();

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

const mockGetHookServerUrl = vi.fn();

vi.mock("@/lib/hookEndpoint", () => ({
  getHookServerUrl: (...args: unknown[]) => mockGetHookServerUrl(...args),
}));

const hookFiles = [
  {
    filePath: "/home/user/project/.claude/hooks/kanvibe-stop-hook.sh",
    content: "#!/bin/bash\necho '작업 완료'\n",
    mode: 0o755,
  },
  {
    filePath: "/home/user/project/.claude/settings.json",
    content: '{\n  "hooks": {}\n}\n',
  },
];

describe("writeHookProviderFiles — 원격 설치", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetHookServerUrl.mockResolvedValue("http://192.168.0.10:9736");
  });

  it("should install through a single short bootstrap command instead of injecting the payload", async () => {
    // Given
    mockExecGit.mockResolvedValue(`${HOOK_INSTALL_SUCCESS_MARKER}\n`);
    const { writeHookProviderFiles } = await import("@/lib/hookFileWriter");

    // When
    await writeHookProviderFiles(hookFiles, "remote-host");

    // Then
    expect(mockExecGit).toHaveBeenCalledTimes(1);
    const [command, sshHost] = mockExecGit.mock.calls[0];
    expect(sshHost).toBe("remote-host");
    expect(Buffer.byteLength(String(command))).toBeLessThan(400);
    expect(String(command)).toContain("/api/install/hooks.sh?token=");
    expect(String(command)).not.toContain("base64 -d");
  });

  it("should fall back to SSH injection when the remote cannot reach the hook server", async () => {
    // Given
    mockExecGit
      .mockRejectedValueOnce(new Error("curl: (7) Failed to connect"))
      .mockResolvedValueOnce("");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { writeHookProviderFiles } = await import("@/lib/hookFileWriter");

    // When
    await writeHookProviderFiles(hookFiles, "remote-host");

    // Then
    expect(mockExecGit).toHaveBeenCalledTimes(2);
    const fallbackCommand = String(mockExecGit.mock.calls[1][0]);
    expect(fallbackCommand).toContain("base64 -d");
    expect(fallbackCommand).toContain("/home/user/project/.claude/hooks/kanvibe-stop-hook.sh");
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("should fall back when the bootstrap script does not report completion", async () => {
    // Given
    mockExecGit
      .mockResolvedValueOnce("sh: curl: not found\n")
      .mockResolvedValueOnce("");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { writeHookProviderFiles } = await import("@/lib/hookFileWriter");

    // When
    await writeHookProviderFiles(hookFiles, "remote-host");

    // Then
    expect(mockExecGit).toHaveBeenCalledTimes(2);
    expect(String(mockExecGit.mock.calls[1][0])).toContain("base64 -d");
    warn.mockRestore();
  });

  it("should not touch the remote at all when there are no files", async () => {
    // Given
    const { writeHookProviderFiles } = await import("@/lib/hookFileWriter");

    // When
    await writeHookProviderFiles([], "remote-host");

    // Then
    expect(mockExecGit).not.toHaveBeenCalled();
  });
});
