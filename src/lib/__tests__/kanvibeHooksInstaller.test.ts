import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TextFileReadResult } from "@/lib/hostFileAccess";
import type { ShellHookProviderFile } from "@/lib/shellHookProvider";

let useRealProviderStatuses = false;
const mockExecGit = vi.fn();
const mockInvalidateOpenCodePluginRegistration = vi.fn();
const mockSetupClaudeHooks = vi.fn();
const mockSetupGeminiHooks = vi.fn();
const mockSetupCodexHooks = vi.fn();
const mockSetupOpenCodeHooks = vi.fn();
const mockGetClaudeHooksStatus = vi.fn();
const mockGetGeminiHooksStatus = vi.fn();
const mockGetCodexHooksStatus = vi.fn();
const mockGetOpenCodeHooksStatus = vi.fn();
const mockGetHookServerUrl = vi.fn();
const mockGetHookServerPort = vi.fn(() => 9736);
const mockGetHookClientId = vi.fn(async () => "own-client");
const mockRegisterKanvibeHookTarget = vi.fn();
const mockReadTextFiles = vi.fn();
const mockWriteHookProviderFiles = vi.fn();

vi.mock("@/lib/openCodePluginRegistry", () => ({
  invalidateOpenCodePluginRegistration: (...args: unknown[]) => mockInvalidateOpenCodePluginRegistration(...args),
  getOpenCodeRegisteredKanvibePluginUrls: vi.fn(),
}));

vi.mock("@/lib/claudeHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/claudeHooksSetup")>()),
  setupClaudeHooks: (...args: unknown[]) => mockSetupClaudeHooks(...args),
  getClaudeHooksStatus: async (...args: Parameters<typeof import("@/lib/claudeHooksSetup").getClaudeHooksStatus>) => useRealProviderStatuses
    ? (await importOriginal<typeof import("@/lib/claudeHooksSetup")>()).getClaudeHooksStatus(...args)
    : mockGetClaudeHooksStatus(...args),
}));

vi.mock("@/lib/geminiHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/geminiHooksSetup")>()),
  setupGeminiHooks: (...args: unknown[]) => mockSetupGeminiHooks(...args),
  getGeminiHooksStatus: async (...args: Parameters<typeof import("@/lib/geminiHooksSetup").getGeminiHooksStatus>) => useRealProviderStatuses
    ? (await importOriginal<typeof import("@/lib/geminiHooksSetup")>()).getGeminiHooksStatus(...args)
    : mockGetGeminiHooksStatus(...args),
}));

vi.mock("@/lib/codexHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/codexHooksSetup")>()),
  setupCodexHooks: (...args: unknown[]) => mockSetupCodexHooks(...args),
  getCodexHooksStatus: async (...args: Parameters<typeof import("@/lib/codexHooksSetup").getCodexHooksStatus>) => useRealProviderStatuses
    ? (await importOriginal<typeof import("@/lib/codexHooksSetup")>()).getCodexHooksStatus(...args)
    : mockGetCodexHooksStatus(...args),
}));

vi.mock("@/lib/openCodeHooksSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/openCodeHooksSetup")>()),
  setupOpenCodeHooks: (...args: unknown[]) => mockSetupOpenCodeHooks(...args),
  getOpenCodeHooksStatus: async (...args: Parameters<typeof import("@/lib/openCodeHooksSetup").getOpenCodeHooksStatus>) => useRealProviderStatuses
    ? (await importOriginal<typeof import("@/lib/openCodeHooksSetup")>()).getOpenCodeHooksStatus(...args)
    : mockGetOpenCodeHooksStatus(...args),
}));

vi.mock("@/lib/gitOperations", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/gitOperations")>()),
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));
vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getOrCreateHookClientId: () => mockGetHookClientId(),
}));

vi.mock("@/lib/hookEndpoint", () => ({
  getHookServerPort: () => mockGetHookServerPort(),
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
  findSymbolicLinkPath: vi.fn(async () => null),
  readTextFile: vi.fn(async () => ""),
  readTextFiles: (...args: unknown[]) => mockReadTextFiles(...args),
  writeTextFile: vi.fn(async () => undefined),
  quoteShellArgument: (value: string) => `'${value}'`,
}));

function buildReadResults(files: Record<string, string> = {}): Map<string, TextFileReadResult> {
  return new Map(Object.entries(files).map(([filePath, content]) => [filePath, { exists: true, content }]));
}

/** 쓰기는 provider 단위로 격리되고 task CLI가 한 배치 더 붙으므로, 한 번의 설치는 provider 수 + 1개의 쓰기 배치를 만든다 */
const HOOK_WRITE_BATCH_COUNT = 5;

/** 마지막 설치가 기록한 파일 전체를 provider 배치들에서 모은다 */
function getLastInstallWrittenFiles(): ShellHookProviderFile[] {
  return mockWriteHookProviderFiles.mock.calls
    .slice(-HOOK_WRITE_BATCH_COUNT)
    .flatMap((call) => (call[0] ?? []) as ShellHookProviderFile[]);
}

function findWrittenContent(filePath: string): string {
  const file = getLastInstallWrittenFiles().find((candidate) => candidate.filePath === filePath);
  if (!file) {
    throw new Error(`write payload not found for ${filePath}`);
  }

  return file.content;
}

function getWrittenFilePaths(): string[] {
  return getLastInstallWrittenFiles().map((file) => file.filePath);
}

/** provider별 쓰기가 모두 시작될 때까지 microtask queue를 비운다 */
async function flushMicrotasks(): Promise<void> {
  for (let tick = 0; tick < 20; tick += 1) {
    await Promise.resolve();
  }
}

describe("kanvibeHooksInstaller", () => {
  it("invalidates OpenCode registration after a bulk file installation", async () => {
    const { installKanvibeHookFiles } = await import("../kanvibeHooksInstaller");
    await installKanvibeHookFiles("/repo", "task-1");
    expect(mockInvalidateOpenCodePluginRegistration).toHaveBeenCalledWith("/repo");
  });

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mockExecGit.mockResolvedValue("");
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

  it("일괄 원격 상태 조회는 한 strict snapshot을 네 provider에 전달하고 쓰지 않는다", async () => {
    const snapshot = buildReadResults({ "/remote/repo/.kanvibe/targets.json": '{"targets":[]}' });
    mockReadTextFiles.mockResolvedValue(snapshot);
    mockGetCodexHooksStatus.mockResolvedValue({ installed: false, hasConfigEntry: false });
    const { getKanvibeHooksStatuses } = await import("../kanvibeHooksInstaller");

    const statuses = await getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host");

    expect(statuses.codex).toEqual({ installed: false, hasConfigEntry: false });
    expect(mockReadTextFiles).toHaveBeenCalledTimes(1);
    expect(mockReadTextFiles).toHaveBeenCalledWith(expect.arrayContaining([
      "/remote/repo/.claude/settings.json", "/remote/repo/.gemini/settings.json",
      "/remote/repo/.codex/config.toml", "/remote/repo/.kanvibe/targets.json",
    ]), "remote-host", { strictHostKeyChecking: true });
    for (const getStatus of [mockGetClaudeHooksStatus, mockGetGeminiHooksStatus, mockGetCodexHooksStatus, mockGetOpenCodeHooksStatus]) {
      expect(getStatus).toHaveBeenCalledWith("/remote/repo", "task-1", "remote-host", snapshot);
    }
    expect(mockRegisterKanvibeHookTarget).not.toHaveBeenCalled();
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
    expect(mockGetHookClientId).not.toHaveBeenCalled();
  });

  it("일괄 원격 상태 조회의 snapshot 실패는 설치 없이 전달한다", async () => {
    mockReadTextFiles.mockRejectedValue(new Error("SSH read failed"));
    const { getKanvibeHooksStatuses } = await import("../kanvibeHooksInstaller");

    await expect(getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host")).rejects.toThrow("SSH read failed");
    expect(mockGetClaudeHooksStatus).not.toHaveBeenCalled();
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
  });

  it("전체 설치는 worktree의 .kanvibe/task.sh를 실행 권한으로 함께 기록한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/repo", "task-1", "remote-host");

    // Then
    const taskCliFile = getLastInstallWrittenFiles().find((file) => file.filePath === "/repo/.kanvibe/task.sh");
    expect(taskCliFile?.mode).toBe(0o755);
    expect(taskCliFile?.content).toContain("/api/cli/tasks");
    expect(mockWriteHookProviderFiles).toHaveBeenCalledWith([taskCliFile], "remote-host");
  });

  it("전체 설치는 provider별 쓰기 배치로 모든 hook 파일을 기록한다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/repo", "task-1", null);

    // Then
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
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

  it("기존 설정 파일은 호스트 키를 검증하는 연결에서 한 번에 읽는다", async () => {
    // Given
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then: 설치 입력 읽기 한 번과 설치 뒤 검증 읽기 한 번이 전부다.
    expect(mockReadTextFiles).toHaveBeenCalledTimes(2);
    expect(mockReadTextFiles).toHaveBeenNthCalledWith(1, [
      "/remote/repo/.claude/settings.json",
      "/remote/repo/.gemini/settings.json",
      "/remote/repo/.codex/config.toml",
      "/remote/repo/.codex/hooks.json",
    ], "remote-host", { strictHostKeyChecking: true });
  });

  it("원격 설치 검증은 provider마다 따로 읽지 않고 한 번 읽은 파일 묶음을 모든 provider에 넘긴다", async () => {
    // Given
    const installInputs = buildReadResults();
    const verificationSnapshot = buildReadResults({ "/remote/repo/.kanvibe/targets.json": '{"targets":[]}' });
    mockReadTextFiles.mockResolvedValueOnce(installInputs).mockResolvedValueOnce(verificationSnapshot);
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    await installKanvibeHooks("/remote/repo", "task-2", "remote-host");

    // Then
    expect(mockReadTextFiles).toHaveBeenNthCalledWith(2, expect.arrayContaining([
      "/remote/repo/.claude/settings.json", "/remote/repo/.kanvibe/targets.json",
    ]), "remote-host", { strictHostKeyChecking: true });
    for (const getStatus of [mockGetClaudeHooksStatus, mockGetGeminiHooksStatus, mockGetCodexHooksStatus, mockGetOpenCodeHooksStatus]) {
      expect(getStatus).toHaveBeenCalledTimes(1);
      expect(getStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host", verificationSnapshot);
    }
  });

  it("검증용 묶음 읽기가 실패하면 provider별로 다시 읽어 실패한 provider만 가려낸다", async () => {
    // Given
    mockReadTextFiles.mockResolvedValueOnce(buildReadResults()).mockRejectedValueOnce(new Error("원격 파일 검사 실패"));
    mockGetCodexHooksStatus.mockRejectedValue(new Error("codex 설정을 읽을 수 없습니다"));
    const { installKanvibeHookFiles, scheduleKanvibeHooksVerification } = await import("@/lib/kanvibeHooksInstaller");
    await installKanvibeHookFiles("/remote/repo", "task-2", "remote-host");
    const onFailure = vi.fn();

    // When
    vi.useFakeTimers();
    try {
      scheduleKanvibeHooksVerification("/remote/repo", "task-2", "remote-host", { onFailure });
      await vi.runAllTimersAsync();
    } finally {
      vi.useRealTimers();
    }

    // Then
    expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
    expect(mockGetCodexHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host");
    expect(onFailure).toHaveBeenCalledTimes(1);
    expect(String(onFailure.mock.calls[0][0])).toContain("Codex");
    expect(String(onFailure.mock.calls[0][0])).not.toContain("Claude");
  });

  it("같은 대상의 상태 조회가 겹치면 원격 읽기 하나를 함께 쓰고, 끝난 뒤의 조회는 다시 읽는다", async () => {
    // Given
    let finishRead!: (files: Map<string, TextFileReadResult>) => void;
    mockReadTextFiles.mockImplementationOnce(() => new Promise((resolve) => { finishRead = resolve; }));
    const { getKanvibeHooksStatuses } = await import("@/lib/kanvibeHooksInstaller");

    // When: 창 두 개가 같은 task의 상태 패널을 동시에 연다
    const first = getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host");
    const second = getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host");
    const otherTask = getKanvibeHooksStatuses("/remote/repo", "task-other", "remote-host");
    await flushMicrotasks();

    // Then: 같은 대상은 읽기 하나, 다른 task는 따로 읽는다
    expect(mockReadTextFiles).toHaveBeenCalledTimes(2);
    finishRead(buildReadResults());
    expect(await first).toEqual(await second);
    await otherTask;
    expect(mockGetClaudeHooksStatus).toHaveBeenCalledTimes(2);

    // 방금 읽은 원격 상태는 30초 동안 다시 읽지 않는다.
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      await getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host");
      expect(mockReadTextFiles).toHaveBeenCalledTimes(2);

      vi.advanceTimersByTime(30_000);
      await getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host");
      expect(mockReadTextFiles).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("로컬 hook 상태는 읽기가 싸므로 매번 새로 읽는다", async () => {
    const { getKanvibeHooksStatuses } = await import("@/lib/kanvibeHooksInstaller");

    await getKanvibeHooksStatuses("/repo", "task-1", null);
    await getKanvibeHooksStatuses("/repo", "task-1", null);

    expect(mockReadTextFiles).toHaveBeenCalledTimes(2);
  });

  it("이 앱이 hook을 설치하면 기억해 둔 원격 상태를 버려 다음 조회가 설치 결과를 읽는다", async () => {
    const { getKanvibeHooksStatuses, installKanvibeHookFiles } = await import("@/lib/kanvibeHooksInstaller");
    mockGetClaudeHooksStatus.mockResolvedValueOnce({ installed: false });
    expect((await getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host")).claude).toEqual({ installed: false });

    await installKanvibeHookFiles("/remote/repo", "task-1", "remote-host");

    expect((await getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host")).claude).toEqual({ installed: true });
  });

  it("설치가 끝나면 설치 전에 시작한 상태 조회를 이후 조회가 이어받지 않는다", async () => {
    // Given: 설치 전에 시작해 아직 끝나지 않은 상태 조회
    let finishStaleRead!: (files: Map<string, TextFileReadResult>) => void;
    mockReadTextFiles.mockImplementationOnce(() => new Promise((resolve) => { finishStaleRead = resolve; }));
    const { getKanvibeHooksStatuses, installKanvibeHookFiles } = await import("@/lib/kanvibeHooksInstaller");
    const staleRead = getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host");
    await flushMicrotasks();
    expect(mockReadTextFiles).toHaveBeenCalledTimes(1);

    // When
    await installKanvibeHookFiles("/remote/repo", "task-1", "remote-host");
    const readCountAfterInstall = mockReadTextFiles.mock.calls.length;
    const freshRead = getKanvibeHooksStatuses("/remote/repo", "task-1", "remote-host");
    await flushMicrotasks();

    // Then: 설치 전 조회가 아직 진행 중이어도 새 조회는 파일을 다시 읽는다
    expect(mockReadTextFiles).toHaveBeenCalledTimes(readCountAfterInstall + 1);
    finishStaleRead(buildReadResults());
    await Promise.all([staleRead, freshRead]);
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
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
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

      expect(mockReadTextFiles).toHaveBeenCalledTimes(1);
      const verificationSnapshot = await mockReadTextFiles.mock.results[0].value;
      expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host", verificationSnapshot);
      expect(mockGetGeminiHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host", verificationSnapshot);
      expect(mockGetCodexHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host", verificationSnapshot);
      expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host", verificationSnapshot);
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

  it.each([
    ["시간 초과", "Command failed: opencode debug config (timed out)"],
    ["CLI 없음", "spawn opencode ENOENT"],
  ])("OpenCode 등록 조회 실패(%s)는 hook 설치를 실패로 처리하지 않는다", async (_reason, registrationError) => {
    // Given
    mockGetOpenCodeHooksStatus.mockResolvedValue({
      installed: false,
      hasPlugin: true,
      pluginRegistrationError: registrationError,
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
    expect(console.warn).toHaveBeenCalledWith("[hooks] OpenCode verification", expect.objectContaining({
      failedChecks: ["hasRegisteredPlugin"],
      pluginRegistrationError: registrationError,
    }));
  });

  it("OpenCode 등록 조회가 실패해도 다른 필수 항목이 빠지면 실패를 전파한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mockGetOpenCodeHooksStatus.mockResolvedValue({
        installed: false,
        hasPlugin: true,
        hasTaskIdBinding: false,
        pluginRegistrationError: "spawn opencode ENOENT",
      });

      const { installKanvibeHookProvider } = await import("@/lib/kanvibeHooksInstaller");

      // When
      const result = expect(installKanvibeHookProvider("/repo", "task-1", "openCode", null)).rejects.toThrow(
        "hooks 검증 실패: OpenCode(hasTaskIdBinding)",
      );
      await vi.runAllTimersAsync();

      // Then
      await result;
    } finally {
      vi.useRealTimers();
    }
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
      expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT * 2);
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
      expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT * 3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("같은 target/task/host 동시 설치 요청은 하나의 설치 작업을 공유한다", async () => {
    // Given
    const pendingHookFileWrites: (() => void)[] = [];
    mockWriteHookProviderFiles.mockImplementation(() => new Promise<void>((resolve) => {
      pendingHookFileWrites.push(resolve);
    }));
    const { installKanvibeHooks } = await import("@/lib/kanvibeHooksInstaller");

    // When
    const firstInstall = installKanvibeHooks("/repo", "task-1", null);
    const secondInstall = installKanvibeHooks("/repo", "task-1", null);
    await flushMicrotasks();

    // Then
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
    for (const resolveHookFileWrite of pendingHookFileWrites) {
      resolveHookFileWrite();
    }
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
      expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
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
    expect(mockGetClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-2", "remote-host", expect.any(Map));
    expect(resolved).toBe(false);

    resolveClaudeVerification({ installed: true });
    await installPromise;
    expect(resolved).toBe(true);
  });
});


describe("kanvibeHooksInstaller conditional migration", () => {
  const repoPath = "/remote/repo";
  const targetsPath = `${repoPath}/.kanvibe/targets.json`;
  const cliPath = `${repoPath}/.kanvibe/task.sh`;
  let files: Map<string, TextFileReadResult>;

  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    useRealProviderStatuses = true;
    mockGetHookClientId.mockResolvedValue("own-client");
    mockGetHookServerPort.mockReturnValue(9736);
    files = new Map();
    mockExecGit.mockResolvedValue("");
    mockGetHookServerUrl.mockResolvedValue("http://127.0.0.1:9736");
    mockReadTextFiles.mockImplementation(async (paths: string[]) => new Map(paths.map((filePath) => [
      filePath, files.get(filePath) ?? { exists: false, content: "" },
    ])));
    mockRegisterKanvibeHookTarget.mockImplementation(async () => {
      const existing = JSON.parse(files.get(targetsPath)?.content ?? '{"targets":[]}');
      const peers = existing.targets.filter((target: { clientId: string }) => target.clientId !== "own-client" && (target as { url?: string }).url !== "http://127.0.0.1:9736");
      files.set(targetsPath, { exists: true, content: JSON.stringify({ targets: [...peers, {
        url: "http://127.0.0.1:9736", taskId: "task-1", token: "token", clientId: "own-client", transport: "ssh",
      }] }) });
    });
    mockWriteHookProviderFiles.mockImplementation(async (writtenFiles: ShellHookProviderFile[]) => {
      writtenFiles.forEach((file) => files.set(file.filePath, { exists: true, content: file.content }));
    });
    const { installKanvibeHooks } = await import("../kanvibeHooksInstaller");
    await installKanvibeHooks(repoPath, "task-1", "remote-host");
    vi.clearAllMocks();
  });

  it("최신 provider와 CLI는 접속을 반복해도 쓰지 않고 파일 변경을 다시 검사한다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
    files.set(cliPath, { exists: true, content: "#!/bin/sh\necho legacy" });
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
    expect(files.get(cliPath)?.content).toContain("KANVIBE_HOOK_TRANSPORT_VERSION=3");
  });

  it("변경 없는 원격 hook은 파일 묶음 한 번만 읽고 provider 재검사를 생략한다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    mockReadTextFiles.mockClear();

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBe(false);

    expect(mockReadTextFiles).toHaveBeenCalledTimes(1);
    expect(mockReadTextFiles.mock.calls[0][0]).toEqual(expect.arrayContaining([...files.keys()]));
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
  });

  it("이전 검증이 성공했어도 provider 파일 삭제를 즉시 발견해 복구한다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    const scriptPath = `${repoPath}/.claude/hooks/kanvibe-prompt-hook.sh`;
    files.delete(scriptPath);

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBe(true);

    expect(files.get(scriptPath)?.content).toContain("KANVIBE_HOOK_TRANSPORT_VERSION=3");
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
  });

  it("검증 결과는 다른 원격 호스트의 검사를 생략하지 않는다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    mockExecGit.mockClear();

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "other-host")).resolves.toBe(false);

    expect(mockExecGit).toHaveBeenCalledWith(expect.any(String), "other-host", expect.any(Object));
  });

  it("이전 검증이 성공했어도 현재 파일 읽기 실패를 성공으로 돌려주지 않는다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    mockReadTextFiles.mockRejectedValueOnce(new Error("SSH read failed"));

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).rejects.toThrow("SSH read failed");
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
  });

  it("검사 도중 파일이 바뀌면 처음 읽은 내용을 검증된 상태로 재사용하지 않는다", async () => {
    const scriptPath = `${repoPath}/.claude/hooks/kanvibe-prompt-hook.sh`;
    let firstSnapshot = true;
    mockReadTextFiles.mockImplementation(async (paths: string[]) => {
      const snapshot = new Map(paths.map((filePath) => [
        filePath, files.get(filePath) ?? { exists: false, content: "" },
      ]));
      if (firstSnapshot && paths.includes(cliPath)) {
        firstSnapshot = false;
        snapshot.set(scriptPath, { exists: false, content: "" });
      }
      return snapshot;
    });
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    files.delete(scriptPath);

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBe(true);
    expect(files.get(scriptPath)?.content).toContain("KANVIBE_HOOK_TRANSPORT_VERSION=3");
  });

  it("검증 후 현재 client 등록이 사라지면 peer를 보존하며 다시 등록한다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    const own = JSON.parse(files.get(targetsPath)!.content).targets[0];
    const peer = { ...own, clientId: "peer-client", url: "http://127.0.0.1:9737" };
    files.set(targetsPath, { exists: true, content: JSON.stringify({ targets: [peer] }) });

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBe(true);
    expect(JSON.parse(files.get(targetsPath)!.content).targets).toContainEqual(peer);
    expect(mockRegisterKanvibeHookTarget).toHaveBeenCalledTimes(1);
  });

  it("현재 client ID가 바뀌면 이전 client의 검증 성공을 재사용하지 않는다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    mockGetHookClientId.mockResolvedValue("new-client");

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).rejects.toThrow("현재 client 등록이 없습니다");
    expect(mockRegisterKanvibeHookTarget).toHaveBeenCalledTimes(1);
  });

  it("다른 task의 등록 누락을 이전 task의 검증으로 감추지 않는다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-2", "remote-host")).rejects.toThrow("hasRegisteredHookTarget");
    expect(mockRegisterKanvibeHookTarget).toHaveBeenCalled();
  });

  it("hook 포트가 바뀌면 파일이 같아도 provider 상태를 다시 검사한다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    mockExecGit.mockClear();
    mockGetHookServerPort.mockReturnValue(19736);

    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBe(false);
    expect(mockExecGit).toHaveBeenCalled();
  });

  it("로컬 provider 상태는 파일이 같아도 매번 새로 검사한다", async () => {
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    useRealProviderStatuses = false;
    const target = JSON.parse(files.get(targetsPath)!.content).targets[0];
    files.set(targetsPath, { exists: true, content: JSON.stringify({ targets: [{ ...target, transport: "local" }] }) });
    [mockGetClaudeHooksStatus, mockGetGeminiHooksStatus, mockGetCodexHooksStatus, mockGetOpenCodeHooksStatus]
      .forEach((getStatus) => getStatus.mockResolvedValue({ installed: true }));

    await ensureKanvibeHooksCurrent(repoPath, "task-1");
    await ensureKanvibeHooksCurrent(repoPath, "task-1");

    expect(mockGetOpenCodeHooksStatus).toHaveBeenCalledTimes(2);
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
  });

  it.each([
    ".claude/hooks/kanvibe-prompt-hook.sh", ".gemini/hooks/kanvibe-stop-hook.sh",
    ".codex/hooks/kanvibe-permission-hook.sh", ".opencode/plugins/kanvibe-plugin.ts",
  ])("구형 provider %s를 최신 전송으로 갱신한다", async (relativePath) => {
    const filePath = `${repoPath}/${relativePath}`;
    const content = files.get(filePath)!.content.replaceAll("KANVIBE_HOOK_TRANSPORT_VERSION=3", "KANVIBE_HOOK_TRANSPORT_VERSION=1");
    files.set(filePath, { exists: true, content });
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
    expect(files.get(filePath)?.content).toContain("KANVIBE_HOOK_TRANSPORT_VERSION=3");
  });

  it.each(["missing", "wrong-transport", "other-client"])("현재 client 등록 %s를 복구하면서 peer를 보존한다", async (scenario) => {
    const ownTarget = JSON.parse(files.get(targetsPath)!.content).targets[0];
    const peer = { ...ownTarget, clientId: "peer-client", url: "http://127.0.0.1:9737" };
    const targets = scenario === "missing" ? [peer] : [peer, {
      ...ownTarget, ...(scenario === "wrong-transport" ? { transport: "local" } : { clientId: "other-client" }),
    }];
    files.set(targetsPath, { exists: true, content: JSON.stringify({ targets }) });
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    expect(mockRegisterKanvibeHookTarget).toHaveBeenCalledTimes(1);
    expect(JSON.parse(files.get(targetsPath)!.content).targets).toContainEqual(peer);
  });

  it("원격 client ID와 transport가 맞으면 예전 IP만으로 다시 설치하지 않는다", async () => {
    const targets = JSON.parse(files.get(targetsPath)!.content).targets;
    targets[0].url = "http://192.0.2.99:9736";
    files.set(targetsPath, { exists: true, content: JSON.stringify({ targets }) });
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBe(false);
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
  });

  it("쓰기 API가 성공해도 CLI가 갱신되지 않으면 검증 실패를 반환한다", async () => {
    files.delete(cliPath);
    mockWriteHookProviderFiles.mockImplementation(async (writtenFiles: ShellHookProviderFile[]) => {
      writtenFiles.filter((file) => file.filePath !== cliPath)
        .forEach((file) => files.set(file.filePath, { exists: true, content: file.content }));
    });
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).rejects.toThrow("task.sh");
  });

  it("터널 도달 실패만으로 최신 파일을 다시 쓰지 않는다", async () => {
    mockExecGit.mockRejectedValue(new Error("SSH disconnected"));
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host");
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
  });

  it("같은 task의 동시 접속은 검사와 설치를 공유한다", async () => {
    files.delete(cliPath);
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await Promise.all(Array.from({ length: 5 }, () => ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")));
    expect(mockRegisterKanvibeHookTarget).toHaveBeenCalledTimes(1);
    expect(mockWriteHookProviderFiles).toHaveBeenCalledTimes(HOOK_WRITE_BATCH_COUNT);
  });

  it("검사 실패는 쓰지 않고 실패하며 다음 접속에서 재시도한다", async () => {
    mockReadTextFiles.mockRejectedValueOnce(new Error("SSH read failed"));
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).rejects.toThrow("SSH read failed");
    expect(mockWriteHookProviderFiles).not.toHaveBeenCalled();
    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBeDefined();
  });

  it("쓰기 실패는 실패로 남고 다음 접속에서 재시도한다", async () => {
    files.delete(cliPath);
    mockWriteHookProviderFiles.mockRejectedValue(new Error("write denied"));
    const { ensureKanvibeHooksCurrent } = await import("../kanvibeHooksInstaller");
    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).rejects.toThrow("write denied");
    mockWriteHookProviderFiles.mockImplementation(async (writtenFiles: ShellHookProviderFile[]) => {
      writtenFiles.forEach((file) => files.set(file.filePath, { exists: true, content: file.content }));
    });
    await expect(ensureKanvibeHooksCurrent(repoPath, "task-1", "remote-host")).resolves.toBeDefined();
    expect(files.get(cliPath)?.content).toContain("KANVIBE_HOOK_TRANSPORT_VERSION=3");
  });
});
