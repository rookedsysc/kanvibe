import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TextFileReadResult } from "@/lib/hostFileAccess";
import type { ShellHookProviderFile } from "@/lib/shellHookProvider";

const mockSetupClaudeHooks = vi.fn();
const mockSetupGeminiHooks = vi.fn();
const mockSetupCodexHooks = vi.fn();
const mockSetupOpenCodeHooks = vi.fn();
const mockGetClaudeHooksStatus = vi.fn();
const mockGetGeminiHooksStatus = vi.fn();
const mockGetCodexHooksStatus = vi.fn();
const mockGetOpenCodeHooksStatus = vi.fn();
const mockGetHookServerUrl = vi.fn();
const mockRegisterKanvibeHookTarget = vi.fn();
const mockReadTextFiles = vi.fn();
const mockWriteHookProviderFiles = vi.fn();

vi.mock("@/lib/claudeHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/claudeHooksSetup")>()),
  setupClaudeHooks: (...args: unknown[]) => mockSetupClaudeHooks(...args),
  getClaudeHooksStatus: (...args: unknown[]) => mockGetClaudeHooksStatus(...args),
}));

vi.mock("@/lib/geminiHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/geminiHooksSetup")>()),
  setupGeminiHooks: (...args: unknown[]) => mockSetupGeminiHooks(...args),
  getGeminiHooksStatus: (...args: unknown[]) => mockGetGeminiHooksStatus(...args),
}));

vi.mock("@/lib/codexHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/codexHooksSetup")>()),
  setupCodexHooks: (...args: unknown[]) => mockSetupCodexHooks(...args),
  getCodexHooksStatus: (...args: unknown[]) => mockGetCodexHooksStatus(...args),
}));

vi.mock("@/lib/openCodeHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/openCodeHooksSetup")>()),
  setupOpenCodeHooks: (...args: unknown[]) => mockSetupOpenCodeHooks(...args),
  getOpenCodeHooksStatus: (...args: unknown[]) => mockGetOpenCodeHooksStatus(...args),
}));

vi.mock("@/lib/hookEndpoint", () => ({
  getHookServerUrl: (...args: unknown[]) => mockGetHookServerUrl(...args),
}));

vi.mock("@/lib/hookTargetRegistration", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/hookTargetRegistration")>()),
  registerKanvibeHookTarget: (...args: unknown[]) => mockRegisterKanvibeHookTarget(...args),
}));

vi.mock("@/lib/hookFileWriter", () => ({
  writeHookProviderFiles: (...args: unknown[]) => mockWriteHookProviderFiles(...args),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  readTextFile: vi.fn(async () => ""),
  readTextFiles: (...args: unknown[]) => mockReadTextFiles(...args),
  writeTextFile: vi.fn(async () => undefined),
  quoteShellArgument: (value: string) => `'${value}'`,
}));

function buildReadResults(files: Record<string, string> = {}): Map<string, TextFileReadResult> {
  return new Map(Object.entries(files).map(([filePath, content]) => [filePath, { exists: true, content }]));
}

/** 마지막 쓰기 배치에서 특정 경로의 파일 내용을 찾는다 */
function findWrittenContent(filePath: string): string {
  const writtenFiles = mockWriteHookProviderFiles.mock.calls.at(-1)?.[0] as ShellHookProviderFile[];
  const file = writtenFiles?.find((candidate) => candidate.filePath === filePath);
  if (!file) {
    throw new Error(`write payload not found for ${filePath}`);
  }

  return file.content;
}

function getWrittenFilePaths(): string[] {
  const writtenFiles = mockWriteHookProviderFiles.mock.calls.at(-1)?.[0] as ShellHookProviderFile[];
  return (writtenFiles ?? []).map((file) => file.filePath);
}

describe("kanvibeHooksInstaller", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mockSetupClaudeHooks.mockResolvedValue(undefined);
    mockSetupGeminiHooks.mockResolvedValue(undefined);
    mockSetupCodexHooks.mockResolvedValue(undefined);
    mockSetupOpenCodeHooks.mockResolvedValue(undefined);
    mockGetHookServerUrl.mockResolvedValue("http://192.168.0.8:9736");
    mockRegisterKanvibeHookTarget.mockResolvedValue(undefined);
    mockReadTextFiles.mockResolvedValue(buildReadResults());
    mockWriteHookProviderFiles.mockResolvedValue(undefined);
    mockGetClaudeHooksStatus.mockResolvedValue({ installed: true });
    mockGetGeminiHooksStatus.mockResolvedValue({ installed: true });
    mockGetCodexHooksStatus.mockResolvedValue({ installed: true });
    mockGetOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  it("전체 설치는 모든 provider hook 파일을 한 번의 쓰기 배치로 기록한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/repo", "task-1", null);

    // Then
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(1);
    expect(mockWriteHookProviderFiles).toHaveBeenCalledWith(expect.any(Array), null);
    expect(getWrittenFilePaths()).toEqual(expect.arrayContaining([
      "/repo/.claude/hooks/kanvibe-prompt-hook.sh",
      "/repo/.claude/settings.json",
      "/repo/.gemini/hooks/kanvibe-stop-hook.sh",
      "/repo/.gemini/settings.json",
      "/repo/.codex/hooks/kanvibe-permission-hook.sh",
      "/repo/.codex/hooks.json",
      "/repo/.codex/config.toml",
      "/repo/.opencode/plugins/kanvibe-plugin.ts",
    ]));
  });

  it("설치는 현재 client를 알림 대상으로 먼저 등록한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/repo", "task-1", null);

    // Then
    expect(mockRegisterKanvibeHookTarget).toHaveBeenCalledWith(
      "/repo",
      "task-1",
      "http://192.168.0.8:9736",
      null,
    );
    expect(mockRegisterKanvibeHookTarget.mock.invocationCallOrder[0]).toBeLessThan(
      mockWriteHookProviderFiles.mock.invocationCallOrder[0],
    );
  });

  it("원격 설치도 같은 경로로 동작하며 sshHost를 쓰기 계층에 전달한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    expect(mockGetHookServerUrl).toHaveBeenCalledWith("remote-host");
    expect(mockWriteHookProviderFiles).toHaveBeenCalledWith(expect.any(Array), "remote-host");
    expect(getWrittenFilePaths()).toContain("/remote/repo/.claude/settings.json");
  });

  it("기존 설정 파일은 provider별로 나눠 읽지 않고 한 번에 읽는다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    expect(mockReadTextFiles).toHaveBeenCalledTimes(1);
    expect(mockReadTextFiles).toHaveBeenCalledWith([
      "/remote/repo/.claude/settings.json",
      "/remote/repo/.gemini/settings.json",
      "/remote/repo/.codex/config.toml",
      "/remote/repo/.codex/hooks.json",
    ], "remote-host");
  });

  it("설치는 제거된 .kanvibe/hooks-targets.json을 읽거나 쓰지 않는다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    const readPaths = (mockReadTextFiles.mock.calls[0]?.[0] ?? []) as string[];
    expect([...readPaths, ...getWrittenFilePaths()].join("\n")).not.toContain("hooks-targets.json");
  });

  it("stale한 Claude/Gemini hook entry는 재설치 시 현재 project 경로로 덮어쓴다", async () => {
    // Given
    mockReadTextFiles.mockResolvedValue(buildReadResults({
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
    }));
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    const claudeSettings = JSON.parse(findWrittenContent("/remote/repo/.claude/settings.json"));
    expect(claudeSettings.hooks.UserPromptSubmit).toHaveLength(1);
    expect(claudeSettings.hooks.UserPromptSubmit[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh');
    expect(claudeSettings.hooks.PreToolUse).toHaveLength(1);
    expect(claudeSettings.hooks.PreToolUse[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh');
    expect(claudeSettings.hooks.Stop).toHaveLength(1);
    expect(claudeSettings.hooks.Stop[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh');

    const geminiSettings = JSON.parse(findWrittenContent("/remote/repo/.gemini/settings.json"));
    expect(geminiSettings.hooks.BeforeAgent).toHaveLength(1);
    expect(geminiSettings.hooks.BeforeAgent[0].hooks[0].command).toBe('"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-prompt-hook.sh');
    expect(geminiSettings.hooks.AfterAgent).toHaveLength(1);
    expect(geminiSettings.hooks.AfterAgent[0].hooks[0].command).toBe('"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-stop-hook.sh');
  });

  it("Codex 재설치는 기존 설정을 보존하면서 최신 hooks.json/config.toml 구조로 갱신한다", async () => {
    // Given
    mockReadTextFiles.mockResolvedValue(buildReadResults({
      "/remote/repo/.codex/config.toml": 'model = "gpt-5"\nnotify = ["other-notify.sh"]\n',
      "/remote/repo/.codex/hooks.json": JSON.stringify({ hooks: { Stop: [{ hooks: [{ type: "command", command: "old-stop" }] }] } }),
    }));
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    const configContent = findWrittenContent("/remote/repo/.codex/config.toml");
    expect(configContent).toContain('model = "gpt-5"');
    expect(configContent).toContain("[features]");
    expect(configContent).toMatch(/^hooks = true$/m);
    expect(configContent).not.toMatch(/^codex_hooks\s*=/m);

    const hooksContent = findWrittenContent("/remote/repo/.codex/hooks.json");
    expect(hooksContent).toContain("UserPromptSubmit");
    expect(hooksContent).toContain("PermissionRequest");
    expect(hooksContent).toContain("PreToolUse");
    expect(hooksContent).toContain("Stop");
  });

  it("hook 파일 설치 API는 provider 검증을 기다리지 않는다", async () => {
    // Given
    const { installKanvibeHookFiles } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHookFiles("/repo", "task-1", null);

    // Then
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(1);
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

      await vi.runAllTimersAsync();

      expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockGetGeminiHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockGetCodexHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
      expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
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
      expect((onFailure.mock.calls[0][0] as Error).message).toContain(
        "hooks 검증 실패: Codex(hasConfigEntry)",
      );
      expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("provider별 설치는 선택한 provider setup만 실행한다", async () => {
    // Given
    const { installKanvibeHookProvider } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHookProvider("/repo", "task-1", "codex", null);

    // Then
    expect(mockSetupCodexHooks).toHaveBeenCalledWith("/repo", "task-1", "http://192.168.0.8:9736", null);
    expect(mockSetupClaudeHooks).not.toHaveBeenCalled();
    expect(mockSetupGeminiHooks).not.toHaveBeenCalled();
    expect(mockSetupOpenCodeHooks).not.toHaveBeenCalled();
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
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
      hasRegisteredHookTarget: true,
      hasStatusEndpoint: true,
      hasEventMappings: true,
      hasMainSessionGuard: true,
      hasDuplicateProgressGuard: true,
    });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When & Then
    await expect(installKanvibeHooks("/repo", "task-1", null)).resolves.toBeUndefined();
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
      hasRegisteredHookTarget: true,
      hasStatusEndpoint: true,
      hasEventMappings: true,
      hasMainSessionGuard: true,
      hasDuplicateProgressGuard: true,
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

  it("hook 파일 쓰기가 일시적으로 실패하면 재시도 후 성공한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockWriteHookProviderFiles
        .mockRejectedValueOnce(new Error("remote host busy"))
        .mockResolvedValue(undefined);
      const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHooks("/repo", "task-1", null)).resolves.toBeUndefined();
      await vi.runAllTimersAsync();

      // Then
      await result;
      expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("hook 파일 쓰기가 계속 실패하면 재시도 후 예외를 전파한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockWriteHookProviderFiles.mockRejectedValue(new Error("remote host unavailable"));
      const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHooks("/remote/repo", "task-2", "remote-host")).rejects.toThrow("remote host unavailable");
      await vi.runAllTimersAsync();

      // Then
      await result;
      expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("같은 target/task/host 동시 설치 요청은 하나의 설치 작업을 공유한다", async () => {
    // Given
    let resolveHookFileWrite: (() => void) | undefined;
    mockWriteHookProviderFiles.mockImplementation(() => new Promise<void>((resolve) => {
      resolveHookFileWrite = resolve;
    }));
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    const firstInstall = installKanvibeHooks("/repo", "task-1", null);
    const secondInstall = installKanvibeHooks("/repo", "task-1", null);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    // Then
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(1);
    resolveHookFileWrite?.();
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
      expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(1);
      expect(onSuccessA).toHaveBeenCalledTimes(1);
      expect(onSuccessB).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("설치 후 provider별 검증 결과를 로그로 남긴다", async () => {
    // Given
    mockGetClaudeHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetGeminiHooksStatus.mockResolvedValue({ installed: true, hasSettingsEntry: true });
    mockGetCodexHooksStatus.mockResolvedValue({ installed: true, hasConfigEntry: true });
    mockGetOpenCodeHooksStatus.mockResolvedValue({ installed: true, hasRegisteredPlugin: true });

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/repo", "task-1", null);

    // Then
    expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/repo", "task-1", null);
    expect(console.log).toHaveBeenCalledWith("[hooks] Claude verification", expect.objectContaining({
      installed: true,
      targetPath: "/repo",
      taskId: "task-1",
    }));
  });

  it("검증에서 미설치 provider가 있으면 설치 실패로 재시도 후 전파한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockGetCodexHooksStatus.mockResolvedValue({ installed: false, hasConfigEntry: false });

      const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHooks("/repo", "task-1", null)).rejects.toThrow(
        "hooks 검증 실패: Codex(hasConfigEntry)",
      );
      await vi.runAllTimersAsync();

      // Then
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

  it("원격 설치는 provider 검증이 끝난 뒤에 반환한다", async () => {
    // Given
    let resolveClaudeVerification: (value: { installed: true }) => void = () => {};
    mockGetClaudeHooksStatus.mockReturnValue(new Promise((resolve) => {
      resolveClaudeVerification = resolve;
    }));

    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    let resolved = false;
    const installPromise = installKanvibeHooks("/remote/repo", "task-2", "remote-host").then(() => {
      resolved = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Then
    expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
    expect(resolved).toBe(false);

    resolveClaudeVerification({ installed: true });
    await installPromise;
    expect(resolved).toBe(true);
  });
});
