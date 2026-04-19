import { beforeEach, describe, expect, it, vi } from "vitest";

const mockSetupClaudeHooks = vi.fn();
const mockSetupGeminiHooks = vi.fn();
const mockSetupCodexHooks = vi.fn();
const mockSetupOpenCodeHooks = vi.fn();
const mockExecGit = vi.fn();
const mockGetHookServerUrl = vi.fn();
const mockGetHookServerToken = vi.fn();

vi.mock("@/lib/claudeHooksSetup", () => ({
  setupClaudeHooks: (...args: unknown[]) => mockSetupClaudeHooks(...args),
  generatePromptHookScript: vi.fn(() => "claude prompt"),
  generateStopHookScript: vi.fn(() => "claude stop"),
  generateQuestionHookScript: vi.fn(() => "claude question"),
}));

vi.mock("@/lib/geminiHooksSetup", () => ({
  setupGeminiHooks: (...args: unknown[]) => mockSetupGeminiHooks(...args),
  generatePromptHookScript: vi.fn(() => "gemini prompt"),
  generateStopHookScript: vi.fn(() => "gemini stop"),
}));

vi.mock("@/lib/codexHooksSetup", () => ({
  setupCodexHooks: (...args: unknown[]) => mockSetupCodexHooks(...args),
  generateNotifyHookScript: vi.fn(() => "codex notify"),
  HOOK_SCRIPT_NAME: "kanvibe-notify-hook.sh",
  CONFIG_FILE_NAME: "config.toml",
}));

vi.mock("@/lib/openCodeHooksSetup", () => ({
  setupOpenCodeHooks: (...args: unknown[]) => mockSetupOpenCodeHooks(...args),
  generatePluginScript: vi.fn(() => "open code plugin"),
  PLUGIN_DIR_NAME: "plugins",
  PLUGIN_FILE_NAME: "kanvibe-plugin.ts",
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

vi.mock("@/lib/hookEndpoint", () => ({
  getHookServerUrl: (...args: unknown[]) => mockGetHookServerUrl(...args),
  getHookServerToken: (...args: unknown[]) => mockGetHookServerToken(...args),
}));

describe("kanvibeHooksInstaller", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mockGetHookServerUrl.mockReturnValue("http://192.168.0.8:9736");
    mockGetHookServerToken.mockReturnValue("token-123");
    mockExecGit.mockResolvedValue("");
  });

  it("로컬 프로젝트면 기존 hook setup 함수들에 서버 URL과 토큰을 전달한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/repo", "task-1", null);

    // Then
    expect(mockSetupClaudeHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736", "token-123");
    expect(mockSetupGeminiHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736", "token-123");
    expect(mockSetupCodexHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736", "token-123");
    expect(mockSetupOpenCodeHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736", "token-123");
    expect(mockExecGit).not.toHaveBeenCalled();
  });

  it("원격 프로젝트면 SSH 명령으로 hook 파일을 설치한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    expect(mockSetupClaudeHooks).not.toHaveBeenCalled();
    expect(mockExecGit).toHaveBeenCalled();
    expect(mockGetHookServerUrl).toHaveBeenCalledWith("remote-host");
  });

  it("원격 hook 설치 중 첫 SSH 쓰기가 실패하면 추가 설치를 진행하지 않는다", async () => {
    // Given
    mockExecGit.mockRejectedValueOnce(new Error("remote host unavailable"));
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    const result = installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    await expect(result).rejects.toThrow("remote host unavailable");
    expect(mockExecGit).toHaveBeenCalledTimes(1);
  });

  it("로컬 hook 설치 중 하나라도 실패하면 예외를 전파한다", async () => {
    // Given
    mockSetupOpenCodeHooks.mockRejectedValueOnce(new Error("open code failed"));
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    const result = installKanvibeHooks("/repo", "task-3", null);

    // Then
    await expect(result).rejects.toThrow("open code failed");
  });
});
