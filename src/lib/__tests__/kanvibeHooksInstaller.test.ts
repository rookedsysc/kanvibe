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
const mockAddAiToolPatternsToGitExclude = vi.fn();
const mockUpsertKanvibeHookTarget = vi.fn();

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
  upsertCodexConfigToml: vi.fn((content: string) => `${content.trimEnd()}\n[features]\nhooks = true\n`),
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
}));

vi.mock("@/lib/gitExclude", () => ({
  addAiToolPatternsToGitExclude: (...args: unknown[]) => mockAddAiToolPatternsToGitExclude(...args),
}));

vi.mock("@/lib/kanvibeProjectState", () => ({
  upsertKanvibeHookTarget: (...args: unknown[]) => mockUpsertKanvibeHookTarget(...args),
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

function buildRemoteTextFileRecords(files: Record<string, string>): string {
  return Object.entries(files)
    .map(([filePath, content]) => [
      "__KANVIBE_FILE_RECORD__",
      filePath,
      "1",
      Buffer.from(content, "utf-8").toString("base64"),
    ].join("\t"))
    .join("\n");
}

describe("kanvibeHooksInstaller", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mockSetupClaudeHooks.mockReset();
    mockSetupGeminiHooks.mockReset();
    mockSetupCodexHooks.mockReset();
    mockSetupOpenCodeHooks.mockReset();
    mockExecGit.mockReset();
    mockGetHookServerUrl.mockReset();
    mockAddAiToolPatternsToGitExclude.mockReset();
    mockUpsertKanvibeHookTarget.mockReset();
    mockSetupClaudeHooks.mockResolvedValue(undefined);
    mockSetupGeminiHooks.mockResolvedValue(undefined);
    mockSetupCodexHooks.mockResolvedValue(undefined);
    mockSetupOpenCodeHooks.mockResolvedValue(undefined);
    mockGetHookServerUrl.mockReturnValue("http://192.168.0.8:9736");
    mockUpsertKanvibeHookTarget.mockResolvedValue(undefined);
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

  it("로컬 프로젝트면 기존 hook setup 함수들에 서버 URL을 전달한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/repo", "task-1", null);

    // Then
    expect(mockSetupClaudeHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockSetupGeminiHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockSetupCodexHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockSetupOpenCodeHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockUpsertKanvibeHookTarget).toHaveBeenCalledWith(
      "/repo",
      { url: "http://192.168.0.8:9736", taskId: "task-1" },
      null,
    );
    expect(mockExecGit).not.toHaveBeenCalled();
  });

  it("hook 파일 설치 API는 provider 검증을 기다리지 않는다", async () => {
    // Given
    const { installKanvibeHookFiles } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHookFiles("/repo", "task-1", null);

    // Then
    expect(mockSetupClaudeHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockSetupGeminiHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockSetupCodexHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockSetupOpenCodeHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockUpsertKanvibeHookTarget).toHaveBeenCalledWith(
      "/repo",
      { url: "http://192.168.0.8:9736", taskId: "task-1" },
      null,
    );
    expect(mockGetClaudeHooksStatus).not.toHaveBeenCalled();
    expect(mockGetGeminiHooksStatus).not.toHaveBeenCalled();
    expect(mockGetCodexHooksStatus).not.toHaveBeenCalled();
    expect(mockGetOpenCodeHooksStatus).not.toHaveBeenCalled();
  });

  it("검증 스케줄러는 hook 파일을 다시 쓰지 않고 provider status만 확인한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      const onSuccess = vi.fn();
      const onFailure = vi.fn();
      mockGetClaudeHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
      mockGetGeminiHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
      mockGetCodexHooksStatus.mockResolvedValue({ installed: true, hasConfigEntry: true });
      mockGetOpenCodeHooksStatus.mockResolvedValue({ installed: true, hasRegisteredPlugin: true });

      const { scheduleKanvibeHooksVerification } = await import("@/lib/kanvibeHooksInstaller");

      // When
      scheduleKanvibeHooksVerification("/remote/repo", "task-2", "remote-host", {
        onSuccess,
        onFailure,
      });

      // Then
      expect(mockGetClaudeHooksStatus).not.toHaveBeenCalled();
      expect(mockSetupClaudeHooks).not.toHaveBeenCalled();

      await vi.runAllTimersAsync();

      expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockGetGeminiHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockGetCodexHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockSetupClaudeHooks).not.toHaveBeenCalled();
      expect(mockSetupGeminiHooks).not.toHaveBeenCalled();
      expect(mockSetupCodexHooks).not.toHaveBeenCalled();
      expect(mockSetupOpenCodeHooks).not.toHaveBeenCalled();
      expect(onSuccess).toHaveBeenCalledTimes(1);
      expect(onFailure).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("검증 스케줄러는 provider 검증 실패를 failure callback으로 전달한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      const onSuccess = vi.fn();
      const onFailure = vi.fn();
      mockGetCodexHooksStatus.mockResolvedValue({ installed: false, hasConfigEntry: false });

      const { scheduleKanvibeHooksVerification } = await import("@/lib/kanvibeHooksInstaller");

      // When
      scheduleKanvibeHooksVerification("/repo", "task-1", null, {
        onSuccess,
        onFailure,
      });
      await vi.runAllTimersAsync();

      // Then
      expect(onSuccess).not.toHaveBeenCalled();
      expect(onFailure).toHaveBeenCalledTimes(1);
      expect(onFailure.mock.calls[0][0]).toEqual(expect.any(Error));
      expect((onFailure.mock.calls[0][0] as Error).message).toContain(
        "hooks 검증 실패: Codex(hasConfigEntry)",
      );
      expect(mockSetupCodexHooks).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("provider별 설치는 선택한 provider setup만 기다리고 다른 provider는 실행하지 않는다", async () => {
    // Given
    const { installKanvibeHookProvider } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHookProvider("/repo", "task-1", "codex", null);

    // Then
    expect(mockSetupCodexHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736");
    expect(mockUpsertKanvibeHookTarget).toHaveBeenCalledWith(
      "/repo",
      { url: "http://192.168.0.8:9736", taskId: "task-1" },
      null,
    );
    expect(mockSetupClaudeHooks).not.toHaveBeenCalled();
    expect(mockSetupGeminiHooks).not.toHaveBeenCalled();
    expect(mockSetupOpenCodeHooks).not.toHaveBeenCalled();
  });

  it("OpenCode 등록만 누락된 상태는 전체 hook 설치 실패로 처리하지 않는다", async () => {
    // Given
    mockGetClaudeHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetGeminiHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetCodexHooksStatus.mockResolvedValue({ installed: true, hasConfigEntry: true });
    mockGetOpenCodeHooksStatus.mockResolvedValue({
      installed: false,
      hasPlugin: true,
      hasRegisteredPlugin: false,
      hasTaskIdBinding: true,
      hasExpectedTaskId: true,
      hasStatusEndpoint: true,
      hasEventMappings: true,
      hasMainSessionGuard: true,
      hasDuplicateProgressGuard: true,
      hasExpectedHookServerUrl: true,
    });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When & Then
    await expect(installKanvibeHooks("/repo", "task-1", null)).resolves.toBeUndefined();
    expect(mockSetupOpenCodeHooks).toHaveBeenCalledTimes(1);
    expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledTimes(1);
    expect(console.warn).toHaveBeenCalledWith("[hooks] OpenCode verification", expect.objectContaining({
      failedChecks: ["hasRegisteredPlugin"],
      installed: false,
      targetPath: "/repo",
    }));
  });

  it("OpenCode 단독 설치도 등록 누락만으로 실패하지 않는다", async () => {
    // Given
    mockGetOpenCodeHooksStatus.mockResolvedValue({
      installed: false,
      hasPlugin: true,
      hasRegisteredPlugin: false,
      hasTaskIdBinding: true,
      hasExpectedTaskId: true,
      hasStatusEndpoint: true,
      hasEventMappings: true,
      hasMainSessionGuard: true,
      hasDuplicateProgressGuard: true,
      hasExpectedHookServerUrl: true,
    });

    const { installKanvibeHookProvider } = await import("@/lib/kanvibeHooksInstaller");

    // When & Then
    await expect(installKanvibeHookProvider("/repo", "task-1", "openCode", null)).resolves.toBeUndefined();
    expect(mockSetupOpenCodeHooks).toHaveBeenCalledTimes(1);
    expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledTimes(1);
  });

  it("OpenCode 단독 설치는 plugin 파일 검증 실패를 전파한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockGetOpenCodeHooksStatus.mockResolvedValue({
        installed: false,
        hasPlugin: false,
        hasRegisteredPlugin: false,
      });

      const { installKanvibeHookProvider } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHookProvider("/repo", "task-1", "openCode", null)).rejects.toThrow(
        "hooks 검증 실패: OpenCode(hasPlugin)",
      );
      await vi.runAllTimersAsync();

      // Then
      await result;
      expect(mockSetupOpenCodeHooks).toHaveBeenCalledTimes(3);
      expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("provider별 설치 실패는 다른 provider setup으로 재시도하지 않는다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockSetupCodexHooks.mockRejectedValue(new Error("codex busy"));
      const { installKanvibeHookProvider } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHookProvider("/repo", "task-1", "codex", null)).rejects.toThrow("codex busy");
      await vi.runAllTimersAsync();

      // Then
      await result;
      expect(mockSetupCodexHooks).toHaveBeenCalledTimes(3);
      expect(mockSetupClaudeHooks).not.toHaveBeenCalled();
      expect(mockSetupGeminiHooks).not.toHaveBeenCalled();
      expect(mockSetupOpenCodeHooks).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("로컬 hook 설치가 일시적으로 실패하면 재시도 후 성공한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockSetupOpenCodeHooks
        .mockRejectedValueOnce(new Error("open code busy"))
        .mockResolvedValueOnce(undefined);
      const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHooks("/repo", "task-1", null)).resolves.toBeUndefined();
      await vi.runAllTimersAsync();

      // Then
      await result;
      expect(mockSetupOpenCodeHooks).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("같은 target/task/host 동시 설치 요청은 하나의 설치 작업을 공유한다", async () => {
    // Given
    let resolveClaudeInstall: (() => void) | undefined;
    mockSetupClaudeHooks.mockImplementation(() => new Promise<void>((resolve) => {
      resolveClaudeInstall = resolve;
    }));
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    const firstInstall = installKanvibeHooks("/repo", "task-1", null);
    const secondInstall = installKanvibeHooks("/repo", "task-1", null);
    await Promise.resolve();
    await Promise.resolve();

    // Then
    expect(mockSetupClaudeHooks).toHaveBeenCalledTimes(1);
    resolveClaudeInstall?.();
    await expect(Promise.all([firstInstall, secondInstall])).resolves.toEqual([undefined, undefined]);
  });

  it("백그라운드 스케줄러는 같은 설치 요청을 합치고 모든 callback을 호출한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      const onSuccessA = vi.fn();
      const onSuccessB = vi.fn();
      const { scheduleKanvibeHooksInstall } = await import("@/lib/kanvibeHooksInstaller");

      // When
      scheduleKanvibeHooksInstall("/repo", "task-1", null, { onSuccess: onSuccessA });
      scheduleKanvibeHooksInstall("/repo", "task-1", null, { onSuccess: onSuccessB });
      await vi.runAllTimersAsync();

      // Then
      expect(mockSetupClaudeHooks).toHaveBeenCalledTimes(1);
      expect(onSuccessA).toHaveBeenCalledTimes(1);
      expect(onSuccessB).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
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

  it("원격 hook 설치는 제거된 .kanvibe/hooks-targets.json을 읽거나 쓰지 않는다", async () => {
    // Given
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("__KANVIBE_FILE_RECORD__")) {
        return buildRemoteTextFileRecords({
          "/remote/repo/.kanvibe/hooks-targets.json": JSON.stringify({
            version: 1,
            targets: [
              { url: "http://10.0.0.5:9736", taskId: "other-client-task" },
            ],
          }),
        });
      }

      return "";
    });
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    const commands = mockExecGit.mock.calls.map(([command]) => String(command));
    expect(commands.join("\n")).not.toContain("/remote/repo/.kanvibe/hooks-targets.json");
  });

  it.each([
    ["claude", "/remote/repo/.claude/settings.json", "/remote/repo/.gemini/settings.json"],
    ["gemini", "/remote/repo/.gemini/settings.json", "/remote/repo/.claude/settings.json"],
    ["codex", "/remote/repo/.codex/hooks.json", "/remote/repo/.opencode/plugins/kanvibe-plugin.ts"],
    ["openCode", "/remote/repo/.opencode/plugins/kanvibe-plugin.ts", "/remote/repo/.codex/hooks.json"],
  ] as const)("원격 %s 단독 hook 설치도 target registry를 갱신하고 provider 파일만 쓴다", async (provider, expectedProviderFile, unexpectedProviderFile) => {
    // Given
    const { installKanvibeHookProvider } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHookProvider("/remote/repo", "task-2", provider, "remote-host");

    // Then
    const writeCommands = mockExecGit.mock.calls
      .map(([command]) => String(command))
      .filter((command) => command.includes("printf '%s'") && command.includes(" > "));
    expect(writeCommands.join("\n")).toContain(expectedProviderFile);
    expect(writeCommands.join("\n")).not.toContain(unexpectedProviderFile);
    expect(writeCommands.join("\n")).not.toContain("/remote/repo/.kanvibe/hooks-targets.json");
    expect(mockUpsertKanvibeHookTarget).toHaveBeenCalledWith(
      "/remote/repo",
      { url: "http://192.168.0.8:9736", taskId: "task-2" },
      "remote-host",
    );
  });

  it("원격 전체 hook 설치는 기존 설정 파일을 한 번의 SSH 명령으로 읽는다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    const batchReadCommands = mockExecGit.mock.calls.filter(([command]) => typeof command === "string"
      && command.includes("__KANVIBE_FILE_RECORD__"));
    const individualReadCommands = mockExecGit.mock.calls.filter(([command]) => typeof command === "string"
      && command.includes(" cat "));
    const writeCommands = mockExecGit.mock.calls.filter(([command]) => typeof command === "string"
      && command.includes("printf '%s'")
      && command.includes(" > "));

    expect(batchReadCommands).toHaveLength(1);
    expect(individualReadCommands).toHaveLength(0);
    expect(writeCommands).toHaveLength(4);
  });

  it("원격 Claude/Gemini stale hook entry도 재설치 시 현재 project 경로로 덮어쓴다", async () => {
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("__KANVIBE_FILE_RECORD__")) {
        return buildRemoteTextFileRecords({
          "/remote/repo/.claude/settings.json": JSON.stringify({
            hooks: {
              UserPromptSubmit: [{ hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-prompt-hook.sh"', timeout: 10 }] }],
              PreToolUse: [{ matcher: "AskUserQuestion", hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-question-hook.sh"', timeout: 10 }] }],
              PostToolUse: [{ matcher: "AskUserQuestion", hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-prompt-hook.sh"', timeout: 10 }] }],
              Stop: [{ hooks: [{ type: "command", command: '"/tmp/old/.claude/hooks/kanvibe-stop-hook.sh"', timeout: 10 }] }],
            },
          }),
          "/remote/repo/.gemini/settings.json": JSON.stringify({
            hooks: {
              BeforeAgent: [{ matcher: "*", hooks: [{ type: "command", command: '"/tmp/old/.gemini/hooks/kanvibe-prompt-hook.sh"', timeout: 10000 }] }],
              AfterAgent: [{ matcher: "*", hooks: [{ type: "command", command: '"/tmp/old/.gemini/hooks/kanvibe-stop-hook.sh"', timeout: 10000 }] }],
            },
          }),
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
      if (command.includes("__KANVIBE_FILE_RECORD__")) {
        return buildRemoteTextFileRecords({
          "/remote/repo/.codex/config.toml": 'model = "gpt-5"\nnotify = ["other-notify.sh"]\n',
          "/remote/repo/.codex/hooks.json": JSON.stringify({ hooks: { Stop: [{ hooks: [{ type: "command", command: "old-stop" }] }] } }),
        });
      }

      return "";
    });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    const configContent = extractWrittenContent(mockExecGit.mock.calls, "/remote/repo/.codex/config.toml");
    expect(configContent).toContain('model = "gpt-5"');
    expect(configContent).toContain("[features]");
    expect(configContent).toMatch(/^hooks = true$/m);
    expect(configContent).not.toMatch(/^codex_hooks\s*=/m);

    const hooksContent = extractWrittenContent(mockExecGit.mock.calls, "/remote/repo/.codex/hooks.json");
    expect(hooksContent).toContain("UserPromptSubmit");
    expect(hooksContent).toContain("PermissionRequest");
    expect(hooksContent).toContain("PreToolUse");
    expect(hooksContent).toContain("Stop");
  });

  it("원격 hook 설치 중 SSH 쓰기가 계속 실패해도 모든 provider 쓰기를 시도한 뒤 예외를 전파한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockExecGit.mockImplementation(async (command: string) => {
        if (command.includes("printf '%s'") && command.includes(" > ")) {
          throw new Error("remote host unavailable");
        }

        return "";
      });
      const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHooks("/remote/repo", "task-2", "remote-host")).rejects.toThrow("remote host unavailable");
      await vi.runAllTimersAsync();

      // Then
      await result;
      const writeCommands = mockExecGit.mock.calls.filter(([command]) => typeof command === "string"
        && command.includes("printf '%s'")
        && command.includes(" > "));
      expect(writeCommands).toHaveLength(12);
    } finally {
      vi.useRealTimers();
    }
  });

  it("로컬 hook 설치 중 하나라도 실패하면 예외를 전파한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockSetupOpenCodeHooks.mockRejectedValue(new Error("open code failed"));
      const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHooks("/repo", "task-3", null)).rejects.toThrow("open code failed");
      await vi.runAllTimersAsync();

      // Then
      await result;
      expect(mockSetupOpenCodeHooks).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("설치 후 provider별 검증 결과를 로그로 남긴다", async () => {
    mockGetClaudeHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetGeminiHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetCodexHooksStatus.mockResolvedValue({ installed: true, hasConfigEntry: true });
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
  });

  it("검증에서 미설치 provider가 있으면 설치 실패로 재시도 후 전파한다", async () => {
    vi.useFakeTimers();

    try {
      mockGetCodexHooksStatus.mockResolvedValue({ installed: false, hasConfigEntry: false });

      const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

      const result = expect(installKanvibeHooks("/repo", "task-1", null)).rejects.toThrow(
        "hooks 검증 실패: Codex(hasConfigEntry)",
      );
      await vi.runAllTimersAsync();

      await result;
      expect(mockGetCodexHooksStatus).toHaveBeenCalledTimes(3);
      expect(console.warn).toHaveBeenCalledWith("[hooks] Codex verification", expect.objectContaining({
        installed: false,
        failedChecks: ["hasConfigEntry"],
        targetPath: "/repo",
      }));
    } finally {
      vi.useRealTimers();
    }
  });

  it("원격 설치는 SSH 기반 검증 로그를 기다린 뒤 반환한다", async () => {
    let resolveClaudeVerification: (value: { installed: true; hasSettingsEntry: true }) => void = () => {};
    mockGetClaudeHooksStatus.mockReturnValue(new Promise((resolve) => {
      resolveClaudeVerification = resolve;
    }));
    mockGetGeminiHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetCodexHooksStatus.mockResolvedValue({ installed: true, hasConfigEntry: true });
    mockGetOpenCodeHooksStatus.mockResolvedValue({ installed: true, hasRegisteredPlugin: true });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    let resolved = false;
    const installPromise = installKanvibeHooks("/remote/repo", "task-2", "remote-host").then(() => {
      resolved = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
    expect(mockGetGeminiHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
    expect(mockGetCodexHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
    expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
    expect(resolved).toBe(false);

    resolveClaudeVerification({ installed: true, hasSettingsEntry: true });
    await installPromise;
    expect(resolved).toBe(true);
  });
});
