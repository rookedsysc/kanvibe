import { beforeEach, describe, expect, it, vi } from "vitest";

const mockSetupClaudeHooks = vi.fn();
const mockSetupGeminiHooks = vi.fn();
const mockSetupCodexHooks = vi.fn();
const mockSetupOpenCodeHooks = vi.fn();
const mockGetClaudeHooksStatus = vi.fn();
const mockGetGeminiHooksStatus = vi.fn();
const mockGetCodexHooksStatus = vi.fn();
const mockGetOpenCodeHooksStatus = vi.fn();
const mockExecGit = vi.fn();
const mockGetHookServerUrl = vi.fn();
const mockGetHookServerToken = vi.fn();
const mockAddAiToolPatternsToGitExclude = vi.fn();

vi.mock("@/lib/claudeHooksSetup", () => ({
  setupClaudeHooks: (...args: unknown[]) => mockSetupClaudeHooks(...args),
  getClaudeHooksStatus: (...args: unknown[]) => mockGetClaudeHooksStatus(...args),
  generatePromptHookScript: vi.fn(() => "claude prompt"),
  generateStopHookScript: vi.fn(() => "claude stop"),
  generateQuestionHookScript: vi.fn(() => "claude question"),
}));

vi.mock("@/lib/geminiHooksSetup", () => ({
  setupGeminiHooks: (...args: unknown[]) => mockSetupGeminiHooks(...args),
  getGeminiHooksStatus: (...args: unknown[]) => mockGetGeminiHooksStatus(...args),
  generatePromptHookScript: vi.fn(() => "gemini prompt"),
  generateStopHookScript: vi.fn(() => "gemini stop"),
}));

vi.mock("@/lib/codexHooksSetup", () => ({
  setupCodexHooks: (...args: unknown[]) => mockSetupCodexHooks(...args),
  getCodexHooksStatus: (...args: unknown[]) => mockGetCodexHooksStatus(...args),
  generatePromptHookScript: vi.fn(() => "codex prompt"),
  generatePermissionHookScript: vi.fn(() => "codex permission"),
  generatePreToolHookScript: vi.fn(() => "codex pre tool"),
  generateStopHookScript: vi.fn(() => "codex stop"),
  upsertCodexConfigToml: vi.fn((content: string) => `${content.trimEnd()}\n[features]\ncodex_hooks = true\n`),
  upsertCodexHooksJson: vi.fn(() => JSON.stringify({ hooks: { UserPromptSubmit: [{}], PermissionRequest: [{}], PreToolUse: [{}], Stop: [{}] } }, null, 2)),
  PROMPT_HOOK_SCRIPT_NAME: "kanvibe-prompt-hook.sh",
  PERMISSION_HOOK_SCRIPT_NAME: "kanvibe-permission-hook.sh",
  PRE_TOOL_HOOK_SCRIPT_NAME: "kanvibe-pre-tool-hook.sh",
  STOP_HOOK_SCRIPT_NAME: "kanvibe-stop-hook.sh",
  HOOKS_FILE_NAME: "hooks.json",
  CONFIG_FILE_NAME: "config.toml",
}));

vi.mock("@/lib/openCodeHooksSetup", () => ({
  setupOpenCodeHooks: (...args: unknown[]) => mockSetupOpenCodeHooks(...args),
  getOpenCodeHooksStatus: (...args: unknown[]) => mockGetOpenCodeHooksStatus(...args),
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

vi.mock("@/lib/gitExclude", () => ({
  addAiToolPatternsToGitExclude: (...args: unknown[]) => mockAddAiToolPatternsToGitExclude(...args),
}));

function extractWrittenContent(calls: unknown[][], filePath: string): string {
  const targetCall = calls.find(([command]) => typeof command === "string"
    && (command.includes(`> "${filePath}"`) || command.includes(`> '${filePath}'`)));
  if (!targetCall) {
    throw new Error(`write command not found for ${filePath}`);
  }

  const command = targetCall[0] as string;
  const escapedFilePath = filePath.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const encodedMatch = command.match(new RegExp(
    `printf '%s' ['"]([^'"]+)['"] \\| \\(base64 -d 2>/dev/null \\|\\| base64 -D\\) > ['"]${escapedFilePath}['"]`,
  ));
  if (!encodedMatch) {
    throw new Error(`base64 payload not found for ${filePath}`);
  }

  return Buffer.from(encodedMatch[1], "base64").toString("utf-8");
}

describe("kanvibeHooksInstaller", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mockGetHookServerUrl.mockReturnValue("http://192.168.0.8:9736");
    mockGetHookServerToken.mockReturnValue("token-123");
    mockExecGit.mockResolvedValue("");
    mockAddAiToolPatternsToGitExclude.mockResolvedValue(undefined);
    mockGetClaudeHooksStatus.mockResolvedValue({ installed: true });
    mockGetGeminiHooksStatus.mockResolvedValue({ installed: true });
    mockGetCodexHooksStatus.mockResolvedValue({ installed: true });
    mockGetOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
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
    expect(mockAddAiToolPatternsToGitExclude).toHaveBeenCalledWith("/remote/repo", "remote-host");
  });

  it("원격 프로젝트면 hooks 파일 설치 전에 git exclude도 함께 갱신한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    expect(mockAddAiToolPatternsToGitExclude).toHaveBeenCalledWith("/remote/repo", "remote-host");
    expect(mockAddAiToolPatternsToGitExclude.mock.invocationCallOrder[0]).toBeLessThan(
      mockExecGit.mock.invocationCallOrder[0],
    );
  });

  it("원격 Claude/Gemini stale hook entry도 재설치 시 현재 project 경로로 덮어쓴다", async () => {
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes('cat "/remote/repo/.claude/settings.json"')) {
        return JSON.stringify({
          hooks: {
            UserPromptSubmit: [{ hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-prompt-hook.sh"', timeout: 10 }] }],
            PreToolUse: [{ matcher: "AskUserQuestion", hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-question-hook.sh"', timeout: 10 }] }],
            PostToolUse: [{ matcher: "AskUserQuestion", hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-prompt-hook.sh"', timeout: 10 }] }],
            Stop: [{ hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-stop-hook.sh"', timeout: 10 }] }],
          },
        });
      }

      if (command.includes('cat "/remote/repo/.gemini/settings.json"')) {
        return JSON.stringify({
          hooks: {
            BeforeAgent: [{ matcher: "*", hooks: [{ type: "command", command: '"/tmp/old/.gemini/hooks/kanvibe-prompt-hook.sh"', timeout: 10000 }] }],
            AfterAgent: [{ matcher: "*", hooks: [{ type: "command", command: '"/tmp/old/.gemini/hooks/kanvibe-stop-hook.sh"', timeout: 10000 }] }],
          },
        });
      }

      return "";
    });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    const claudeSettings = JSON.parse(extractWrittenContent(mockExecGit.mock.calls, "/remote/repo/.claude/settings.json"));
    expect(claudeSettings.hooks.UserPromptSubmit).toHaveLength(1);
    expect(claudeSettings.hooks.UserPromptSubmit[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh');
    expect(claudeSettings.hooks.PreToolUse).toHaveLength(1);
    expect(claudeSettings.hooks.PreToolUse[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh');
    expect(claudeSettings.hooks.Stop).toHaveLength(1);
    expect(claudeSettings.hooks.Stop[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh');

    const geminiSettings = JSON.parse(extractWrittenContent(mockExecGit.mock.calls, "/remote/repo/.gemini/settings.json"));
    expect(geminiSettings.hooks.BeforeAgent).toHaveLength(1);
    expect(geminiSettings.hooks.BeforeAgent[0].hooks[0].command).toBe('"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-prompt-hook.sh');
    expect(geminiSettings.hooks.AfterAgent).toHaveLength(1);
    expect(geminiSettings.hooks.AfterAgent[0].hooks[0].command).toBe('"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-stop-hook.sh');
  });

  it("원격 Codex 재설치는 최신 hooks.json/config.toml 구조로 갱신한다", async () => {
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes('cat "/remote/repo/.codex/config.toml"')) {
        return 'model = "gpt-5"\nnotify = ["other-notify.sh"]\n';
      }

      if (command.includes('cat "/remote/repo/.codex/hooks.json"')) {
        return JSON.stringify({ hooks: { Stop: [{ hooks: [{ type: "command", command: "old-stop" }] }] } });
      }

      return "";
    });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    const configContent = extractWrittenContent(mockExecGit.mock.calls, "/remote/repo/.codex/config.toml");
    expect(configContent).toContain("[features]");
    expect(configContent).toContain("codex_hooks = true");

    const hooksContent = extractWrittenContent(mockExecGit.mock.calls, "/remote/repo/.codex/hooks.json");
    expect(hooksContent).toContain("UserPromptSubmit");
    expect(hooksContent).toContain("PermissionRequest");
    expect(hooksContent).toContain("PreToolUse");
    expect(hooksContent).toContain("Stop");
  });

  it("원격 hook 설치 중 첫 SSH 쓰기가 실패하면 추가 설치를 진행하지 않는다", async () => {
    // Given
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("printf '%s'")) {
        throw new Error("remote host unavailable");
      }

      return "";
    });
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    const result = installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    await expect(result).rejects.toThrow("remote host unavailable");
    expect(mockExecGit).toHaveBeenCalledTimes(2);
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

  it("설치 후 provider별 검증 결과를 로그로 남긴다", async () => {
    mockGetClaudeHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetGeminiHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetCodexHooksStatus.mockResolvedValue({ installed: false, hasConfigEntry: false });
    mockGetOpenCodeHooksStatus.mockResolvedValue({ installed: true, hasRegisteredPlugin: true });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    await installKanvibeHooks("/repo", "task-1", null);

    expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/repo", "task-1", null);
    expect(mockGetGeminiHooksStatus).toHaveBeenCalledWith("/repo", "task-1", null);
    expect(mockGetCodexHooksStatus).toHaveBeenCalledWith("/repo", "task-1", null);
    expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledWith("/repo", "task-1", null);
    expect(console.log).toHaveBeenCalledWith("[hooks] Claude verification", expect.objectContaining({
      installed: true,
      targetPath: "/repo",
      taskId: "task-1",
    }));
    expect(console.warn).toHaveBeenCalledWith("[hooks] Codex verification", expect.objectContaining({
      installed: false,
      failedChecks: ["hasConfigEntry"],
      targetPath: "/repo",
    }));
  });

  it("원격 설치는 후속 검증을 기다리지 않고 반환한다", async () => {
    const pendingVerification = new Promise(() => {});
    mockGetClaudeHooksStatus.mockReturnValue(pendingVerification);
    mockGetGeminiHooksStatus.mockReturnValue(pendingVerification);
    mockGetCodexHooksStatus.mockReturnValue(pendingVerification);
    mockGetOpenCodeHooksStatus.mockReturnValue(pendingVerification);

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    await expect(Promise.race([
      installKanvibeHooks("/remote/repo", "task-2", "remote-host").then(() => "resolved"),
      new Promise((resolve) => setTimeout(() => resolve("timed-out"), 50)),
    ])).resolves.toBe("resolved");
  });
});
