import path from "node:path";
import { execGit } from "@/lib/gitOperations";
import { setupClaudeHooks, getClaudeHooksStatus, generatePromptHookScript as generateClaudePromptHookScript, generateStopHookScript as generateClaudeStopHookScript, generateQuestionHookScript as generateClaudeQuestionHookScript, type ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import { setupGeminiHooks, getGeminiHooksStatus, generatePromptHookScript as generateGeminiPromptHookScript, generateStopHookScript as generateGeminiStopHookScript, type GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import {
  setupCodexHooks,
  getCodexHooksStatus,
  generatePromptHookScript as generateCodexPromptHookScript,
  generatePermissionHookScript as generateCodexPermissionHookScript,
  generatePreToolHookScript as generateCodexPreToolHookScript,
  generateStopHookScript as generateCodexStopHookScript,
  upsertCodexConfigToml,
  upsertCodexHooksJson,
  PROMPT_HOOK_SCRIPT_NAME,
  PERMISSION_HOOK_SCRIPT_NAME,
  PRE_TOOL_HOOK_SCRIPT_NAME,
  STOP_HOOK_SCRIPT_NAME,
  HOOKS_FILE_NAME,
  CONFIG_FILE_NAME,
  type CodexHooksStatus,
} from "@/lib/codexHooksSetup";
import { setupOpenCodeHooks, getOpenCodeHooksStatus, generatePluginScript, PLUGIN_DIR_NAME, PLUGIN_FILE_NAME, type OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";
import { getHookServerUrl } from "@/lib/hookEndpoint";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { quoteShellArgument, readTextFiles } from "@/lib/hostFileAccess";
import { buildKanvibeHookTargetsContent, getKanvibeHookTargetsPath } from "@/lib/kanvibeProjectState";

const HOOK_INSTALL_MAX_ATTEMPTS = 3;
const HOOK_INSTALL_RETRY_DELAY_MS = 500;
const NON_BLOCKING_HOOK_STATUS_CHECKS = new Set([
  "hasReachableHookServer",
  "hasDuplicateKanvibePlugins",
]);
const OPEN_CODE_NON_BLOCKING_INSTALL_CHECKS = new Set([
  "hasRegisteredPlugin",
]);
const activeHookInstallJobs = new Map<string, Promise<void>>();
const activeHookFileInstallJobs = new Map<string, Promise<void>>();
const scheduledHookInstallJobs = new Map<string, ScheduledHookInstallJob>();

export type KanvibeHookProvider = "claude" | "gemini" | "codex" | "openCode";

interface HookInstallScheduleOptions {
  delayMs?: number;
  onSuccess?: () => void;
  onFailure?: (error: unknown) => void;
}

interface HookInstallCallbacks {
  onSuccess?: () => void;
  onFailure?: (error: unknown) => void;
}

interface ScheduledHookInstallJob {
  callbacks: HookInstallCallbacks[];
}

export async function installKanvibeHooks(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const installKey = buildHookInstallKey(targetPath, taskId, sshHost, "all");
  const activeJob = activeHookInstallJobs.get(installKey);
  if (activeJob) {
    return activeJob;
  }

  const installJob = runKanvibeHooksInstallWithRetry(targetPath, taskId, sshHost)
    .finally(() => {
      if (activeHookInstallJobs.get(installKey) === installJob) {
        activeHookInstallJobs.delete(installKey);
      }
    });

  activeHookInstallJobs.set(installKey, installJob);
  return installJob;
}

export async function installKanvibeHookFiles(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const installKey = buildHookInstallKey(targetPath, taskId, sshHost, "all-files");
  const activeJob = activeHookFileInstallJobs.get(installKey);
  if (activeJob) {
    return activeJob;
  }

  const installJob = runKanvibeHookFilesInstallWithRetry(targetPath, taskId, sshHost)
    .finally(() => {
      if (activeHookFileInstallJobs.get(installKey) === installJob) {
        activeHookFileInstallJobs.delete(installKey);
      }
    });

  activeHookFileInstallJobs.set(installKey, installJob);
  return installJob;
}

export async function installKanvibeHookProvider(
  targetPath: string,
  taskId: string,
  provider: KanvibeHookProvider,
  sshHost?: string | null,
): Promise<void> {
  const installKey = buildHookInstallKey(targetPath, taskId, sshHost, provider);
  const activeJob = activeHookInstallJobs.get(installKey);
  if (activeJob) {
    return activeJob;
  }

  const installJob = runKanvibeHookProviderInstallWithRetry(targetPath, taskId, provider, sshHost)
    .finally(() => {
      if (activeHookInstallJobs.get(installKey) === installJob) {
        activeHookInstallJobs.delete(installKey);
      }
    });

  activeHookInstallJobs.set(installKey, installJob);
  return installJob;
}

export function scheduleKanvibeHooksInstall(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
  options: HookInstallScheduleOptions = {},
): void {
  const installKey = buildHookInstallKey(targetPath, taskId, sshHost, "all");
  const callbacks: HookInstallCallbacks = {
    onSuccess: options.onSuccess,
    onFailure: options.onFailure,
  };
  const scheduledJob = scheduledHookInstallJobs.get(installKey);
  if (scheduledJob) {
    scheduledJob.callbacks.push(callbacks);
    return;
  }

  const nextJob: ScheduledHookInstallJob = {
    callbacks: [callbacks],
  };
  scheduledHookInstallJobs.set(installKey, nextJob);

  setTimeout(() => {
    void installKanvibeHooks(targetPath, taskId, sshHost)
      .then(() => notifyHookInstallSuccess(nextJob.callbacks))
      .catch((error) => notifyHookInstallFailure(nextJob.callbacks, error))
      .finally(() => {
        if (scheduledHookInstallJobs.get(installKey) === nextJob) {
          scheduledHookInstallJobs.delete(installKey);
        }
      });
  }, options.delayMs ?? 0);
}

export function scheduleKanvibeHooksVerification(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
  options: HookInstallScheduleOptions = {},
): void {
  setTimeout(() => {
    void verifyHookInstallation(targetPath, taskId, sshHost)
      .then(() => {
        runHookInstallCallback(() => options.onSuccess?.());
      })
      .catch((error) => {
        runHookInstallCallback(() => options.onFailure?.(error));
      });
  }, options.delayMs ?? 0);
}

function buildHookInstallKey(
  targetPath: string,
  taskId: string,
  sshHost: string | null | undefined,
  provider: KanvibeHookProvider | "all" | "all-files",
): string {
  return [provider, sshHost ?? "", targetPath, taskId].join("\0");
}

function notifyHookInstallSuccess(callbacks: HookInstallCallbacks[]): void {
  for (const callback of callbacks) {
    runHookInstallCallback(() => callback.onSuccess?.());
  }
}

function notifyHookInstallFailure(callbacks: HookInstallCallbacks[], error: unknown): void {
  for (const callback of callbacks) {
    runHookInstallCallback(() => callback.onFailure?.(error));
  }
}

function runHookInstallCallback(callback: () => void): void {
  try {
    callback();
  } catch (error) {
    console.warn("[hooks] install callback failed", {
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

async function runKanvibeHooksInstallWithRetry(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  let lastError: unknown;

  for (let attempt = 1; attempt <= HOOK_INSTALL_MAX_ATTEMPTS; attempt += 1) {
    try {
      await installKanvibeHooksOnce(targetPath, taskId, sshHost);
      return;
    } catch (error) {
      lastError = error;

      if (attempt === HOOK_INSTALL_MAX_ATTEMPTS) {
        throw error;
      }

      const retryDelayMs = HOOK_INSTALL_RETRY_DELAY_MS * attempt;
      console.warn("[hooks] install failed; retrying", {
        targetPath,
        taskId,
        sshHost: sshHost ?? null,
        attempt,
        maxAttempts: HOOK_INSTALL_MAX_ATTEMPTS,
        nextAttemptInMs: retryDelayMs,
        error: error instanceof Error ? error.message : String(error),
      });
      await waitForHookInstallRetry(retryDelayMs);
    }
  }

  throw lastError instanceof Error ? lastError : new Error("hooks 설정 실패");
}

async function runKanvibeHookFilesInstallWithRetry(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  let lastError: unknown;

  for (let attempt = 1; attempt <= HOOK_INSTALL_MAX_ATTEMPTS; attempt += 1) {
    try {
      await installKanvibeHookFilesOnce(targetPath, taskId, sshHost);
      return;
    } catch (error) {
      lastError = error;

      if (attempt === HOOK_INSTALL_MAX_ATTEMPTS) {
        throw error;
      }

      const retryDelayMs = HOOK_INSTALL_RETRY_DELAY_MS * attempt;
      console.warn("[hooks] file install failed; retrying", {
        targetPath,
        taskId,
        sshHost: sshHost ?? null,
        attempt,
        maxAttempts: HOOK_INSTALL_MAX_ATTEMPTS,
        nextAttemptInMs: retryDelayMs,
        error: error instanceof Error ? error.message : String(error),
      });
      await waitForHookInstallRetry(retryDelayMs);
    }
  }

  throw lastError instanceof Error ? lastError : new Error("hooks 파일 설정 실패");
}

async function runKanvibeHookProviderInstallWithRetry(
  targetPath: string,
  taskId: string,
  provider: KanvibeHookProvider,
  sshHost?: string | null,
): Promise<void> {
  let lastError: unknown;

  for (let attempt = 1; attempt <= HOOK_INSTALL_MAX_ATTEMPTS; attempt += 1) {
    try {
      await installKanvibeHookProviderOnce(targetPath, taskId, provider, sshHost);
      return;
    } catch (error) {
      lastError = error;

      if (attempt === HOOK_INSTALL_MAX_ATTEMPTS) {
        throw error;
      }

      const retryDelayMs = HOOK_INSTALL_RETRY_DELAY_MS * attempt;
      console.warn("[hooks] provider install failed; retrying", {
        provider,
        targetPath,
        taskId,
        sshHost: sshHost ?? null,
        attempt,
        maxAttempts: HOOK_INSTALL_MAX_ATTEMPTS,
        nextAttemptInMs: retryDelayMs,
        error: error instanceof Error ? error.message : String(error),
      });
      await waitForHookInstallRetry(retryDelayMs);
    }
  }

  throw lastError instanceof Error ? lastError : new Error("hooks 설정 실패");
}

async function waitForHookInstallRetry(delayMs: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, delayMs));
}

interface HookProviderInstaller {
  provider: KanvibeHookProvider;
  label: string;
  install: () => Promise<void>;
  verify: () => Promise<HookVerificationStatus>;
}

function createHookProviderInstallers(
  targetPath: string,
  taskId: string,
  hookServerUrl: string,
  sshHost?: string | null,
): Record<KanvibeHookProvider, HookProviderInstaller> {
  return {
    claude: {
      provider: "claude",
      label: "Claude",
      install: () => sshHost
        ? setupRemoteClaudeHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupClaudeHooks(targetPath, taskId, hookServerUrl),
      verify: () => getClaudeHooksStatus(targetPath, taskId, sshHost),
    },
    gemini: {
      provider: "gemini",
      label: "Gemini",
      install: () => sshHost
        ? setupRemoteGeminiHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupGeminiHooks(targetPath, taskId, hookServerUrl),
      verify: () => getGeminiHooksStatus(targetPath, taskId, sshHost),
    },
    codex: {
      provider: "codex",
      label: "Codex",
      install: () => sshHost
        ? setupRemoteCodexHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupCodexHooks(targetPath, taskId, hookServerUrl),
      verify: () => getCodexHooksStatus(targetPath, taskId, sshHost),
    },
    openCode: {
      provider: "openCode",
      label: "OpenCode",
      install: () => sshHost
        ? setupRemoteOpenCodeHooks(targetPath, taskId, hookServerUrl, sshHost)
        : setupOpenCodeHooks(targetPath, taskId, hookServerUrl),
      verify: () => getOpenCodeHooksStatus(targetPath, taskId, sshHost),
    },
  };
}

async function installKanvibeHooksOnce(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  await installKanvibeHookFilesOnce(targetPath, taskId, sshHost);
  await verifyHookInstallation(targetPath, taskId, sshHost);
}

async function installKanvibeHookFilesOnce(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);

  if (!sshHost) {
    const installers = Object.values(createHookProviderInstallers(targetPath, taskId, hookServerUrl, sshHost));
    const results = await Promise.allSettled(installers.map(({ install }) => install()));
    assertHookInstallResults(results, installers.map(({ label }) => label));
  } else {
    try {
      await addAiToolPatternsToGitExclude(targetPath, sshHost);
    } catch (error) {
      console.warn("[hooks] remote git exclude update failed", {
        targetPath,
        sshHost,
        error: error instanceof Error ? error.message : String(error),
      });
    }

    await setupRemoteKanvibeHooks(targetPath, taskId, hookServerUrl, sshHost);
  }
}

async function installKanvibeHookProviderOnce(
  targetPath: string,
  taskId: string,
  provider: KanvibeHookProvider,
  sshHost?: string | null,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);
  const installer = createHookProviderInstallers(targetPath, taskId, hookServerUrl, sshHost)[provider];

  if (sshHost) {
    try {
      await addAiToolPatternsToGitExclude(targetPath, sshHost);
    } catch (error) {
      console.warn("[hooks] remote git exclude update failed", {
        provider: installer.label,
        targetPath,
        sshHost,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }

  await installer.install();
  await verifyHookProviderInstallation(installer, targetPath, taskId, sshHost);
}

async function setupRemoteClaudeHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const claudeDir = path.posix.join(repoPath, ".claude");
  const settingsPath = path.posix.join(claudeDir, "settings.json");
  const hookTargetsPath = getKanvibeHookTargetsPath(repoPath, sshHost);
  const files = await readTextFiles([settingsPath, hookTargetsPath], sshHost);

  await writeRemoteTextFiles(
    [
      ...buildRemoteClaudeHookFiles(repoPath, taskId, hookServerUrl, files.get(settingsPath)?.content ?? ""),
      buildRemoteHookTargetsFile(repoPath, taskId, hookServerUrl, sshHost, files.get(hookTargetsPath)?.content ?? ""),
    ],
    sshHost,
  );
}

async function setupRemoteGeminiHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const geminiDir = path.posix.join(repoPath, ".gemini");
  const settingsPath = path.posix.join(geminiDir, "settings.json");
  const hookTargetsPath = getKanvibeHookTargetsPath(repoPath, sshHost);
  const files = await readTextFiles([settingsPath, hookTargetsPath], sshHost);

  await writeRemoteTextFiles(
    [
      ...buildRemoteGeminiHookFiles(repoPath, taskId, hookServerUrl, files.get(settingsPath)?.content ?? ""),
      buildRemoteHookTargetsFile(repoPath, taskId, hookServerUrl, sshHost, files.get(hookTargetsPath)?.content ?? ""),
    ],
    sshHost,
  );
}

async function setupRemoteCodexHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const codexDir = path.posix.join(repoPath, ".codex");
  const configPath = path.posix.join(codexDir, CONFIG_FILE_NAME);
  const hooksPath = path.posix.join(codexDir, HOOKS_FILE_NAME);
  const hookTargetsPath = getKanvibeHookTargetsPath(repoPath, sshHost);
  const files = await readTextFiles([configPath, hooksPath, hookTargetsPath], sshHost);

  await writeRemoteTextFiles(
    [
      ...buildRemoteCodexHookFiles(
        repoPath,
        taskId,
        hookServerUrl,
        files.get(configPath)?.content ?? "",
        files.get(hooksPath)?.content ?? "",
      ),
      buildRemoteHookTargetsFile(repoPath, taskId, hookServerUrl, sshHost, files.get(hookTargetsPath)?.content ?? ""),
    ],
    sshHost,
  );
}

async function setupRemoteOpenCodeHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const hookTargetsPath = getKanvibeHookTargetsPath(repoPath, sshHost);
  const files = await readTextFiles([hookTargetsPath], sshHost);
  await writeRemoteTextFiles([
    ...buildRemoteOpenCodeHookFiles(repoPath, taskId, hookServerUrl),
    buildRemoteHookTargetsFile(repoPath, taskId, hookServerUrl, sshHost, files.get(hookTargetsPath)?.content ?? ""),
  ], sshHost);
}

async function setupRemoteKanvibeHooks(repoPath: string, taskId: string, hookServerUrl: string, sshHost: string) {
  const claudeSettingsPath = path.posix.join(repoPath, ".claude", "settings.json");
  const geminiSettingsPath = path.posix.join(repoPath, ".gemini", "settings.json");
  const codexConfigPath = path.posix.join(repoPath, ".codex", CONFIG_FILE_NAME);
  const codexHooksPath = path.posix.join(repoPath, ".codex", HOOKS_FILE_NAME);
  const hookTargetsPath = getKanvibeHookTargetsPath(repoPath, sshHost);
  const existingFiles = await readTextFiles([
    claudeSettingsPath,
    geminiSettingsPath,
    codexConfigPath,
    codexHooksPath,
    hookTargetsPath,
  ], sshHost);
  const plans = [
    {
      label: "Claude",
      files: buildRemoteClaudeHookFiles(repoPath, taskId, hookServerUrl, existingFiles.get(claudeSettingsPath)?.content ?? ""),
    },
    {
      label: "Gemini",
      files: buildRemoteGeminiHookFiles(repoPath, taskId, hookServerUrl, existingFiles.get(geminiSettingsPath)?.content ?? ""),
    },
    {
      label: "Codex",
      files: buildRemoteCodexHookFiles(
        repoPath,
        taskId,
        hookServerUrl,
        existingFiles.get(codexConfigPath)?.content ?? "",
        existingFiles.get(codexHooksPath)?.content ?? "",
      ),
    },
    {
      label: "OpenCode",
      files: buildRemoteOpenCodeHookFiles(repoPath, taskId, hookServerUrl),
    },
    {
      label: "KanVibe targets",
      files: [buildRemoteHookTargetsFile(
        repoPath,
        taskId,
        hookServerUrl,
        sshHost,
        existingFiles.get(hookTargetsPath)?.content ?? "",
      )],
    },
  ];

  const results = await Promise.allSettled(plans.map(async ({ label, files }) => {
    try {
      await writeRemoteTextFiles(files, sshHost);
    } catch (error) {
      console.error(`[hooks] ${label} install failed`, {
        targetPath: repoPath,
        taskId,
        sshHost,
        error: error instanceof Error ? error.message : String(error),
      });
      throw error;
    }
  }));
  assertHookInstallResults(results, plans.map(({ label }) => label));
}

function buildRemoteClaudeHookFiles(
  repoPath: string,
  taskId: string,
  hookServerUrl: string,
  settingsContent: string,
): RemoteTextFile[] {
  const claudeDir = path.posix.join(repoPath, ".claude");
  const hooksDir = path.posix.join(claudeDir, "hooks");
  const settingsPath = path.posix.join(claudeDir, "settings.json");
  const settings = parseRemoteJsonObject(settingsContent);
  const hooks = ((settings.hooks as Record<string, unknown[]>) || {});
  settings.hooks = hooks;

  upsertHookEntry(hooks, "UserPromptSubmit", "kanvibe-prompt-hook.sh", {
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh', timeout: 10 }],
  });
  upsertHookEntry(hooks, "PreToolUse", "kanvibe-question-hook.sh", {
    matcher: "AskUserQuestion",
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh', timeout: 10 }],
  });
  upsertHookEntry(hooks, "PostToolUse", "kanvibe-prompt-hook.sh", {
    matcher: "AskUserQuestion",
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh', timeout: 10 }],
  });
  upsertHookEntry(hooks, "Stop", "kanvibe-stop-hook.sh", {
    hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh', timeout: 10 }],
  });

  return [
    {
      filePath: path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"),
      content: generateClaudePromptHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, "kanvibe-stop-hook.sh"),
      content: generateClaudeStopHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, "kanvibe-question-hook.sh"),
      content: generateClaudeQuestionHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: settingsPath,
      content: JSON.stringify(settings, null, 2) + "\n",
    },
  ];
}

function buildRemoteGeminiHookFiles(
  repoPath: string,
  taskId: string,
  hookServerUrl: string,
  settingsContent: string,
): RemoteTextFile[] {
  const geminiDir = path.posix.join(repoPath, ".gemini");
  const hooksDir = path.posix.join(geminiDir, "hooks");
  const settingsPath = path.posix.join(geminiDir, "settings.json");
  const settings = parseRemoteJsonObject(settingsContent);
  const hooks = ((settings.hooks as Record<string, unknown[]>) || {});
  settings.hooks = hooks;

  upsertHookEntry(hooks, "BeforeAgent", "kanvibe-prompt-hook.sh", {
    matcher: "*",
    hooks: [{ type: "command", command: '"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-prompt-hook.sh', timeout: 10000 }],
  });
  upsertHookEntry(hooks, "AfterAgent", "kanvibe-stop-hook.sh", {
    matcher: "*",
    hooks: [{ type: "command", command: '"$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-stop-hook.sh', timeout: 10000 }],
  });

  return [
    {
      filePath: path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"),
      content: generateGeminiPromptHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, "kanvibe-stop-hook.sh"),
      content: generateGeminiStopHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: settingsPath,
      content: JSON.stringify(settings, null, 2) + "\n",
    },
  ];
}

function buildRemoteCodexHookFiles(
  repoPath: string,
  taskId: string,
  hookServerUrl: string,
  configContent: string,
  hooksContent: string,
): RemoteTextFile[] {
  const codexDir = path.posix.join(repoPath, ".codex");
  const hooksDir = path.posix.join(codexDir, "hooks");
  const configPath = path.posix.join(codexDir, CONFIG_FILE_NAME);
  const hooksPath = path.posix.join(codexDir, HOOKS_FILE_NAME);

  return [
    {
      filePath: path.posix.join(hooksDir, PROMPT_HOOK_SCRIPT_NAME),
      content: generateCodexPromptHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, PERMISSION_HOOK_SCRIPT_NAME),
      content: generateCodexPermissionHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, PRE_TOOL_HOOK_SCRIPT_NAME),
      content: generateCodexPreToolHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: path.posix.join(hooksDir, STOP_HOOK_SCRIPT_NAME),
      content: generateCodexStopHookScript(hookServerUrl, taskId),
      mode: 0o755,
    },
    {
      filePath: configPath,
      content: upsertCodexConfigToml(configContent),
    },
    {
      filePath: hooksPath,
      content: upsertCodexHooksJson(hooksContent),
    },
  ];
}

function buildRemoteOpenCodeHookFiles(
  repoPath: string,
  taskId: string,
  hookServerUrl: string,
): RemoteTextFile[] {
  const pluginDir = path.posix.join(repoPath, ".opencode", PLUGIN_DIR_NAME);
  return [
    {
      filePath: path.posix.join(pluginDir, PLUGIN_FILE_NAME),
      content: generatePluginScript(hookServerUrl, taskId),
    },
  ];
}

function buildRemoteHookTargetsFile(
  repoPath: string,
  taskId: string,
  hookServerUrl: string,
  sshHost: string,
  currentContent: string,
): RemoteTextFile {
  return {
    filePath: getKanvibeHookTargetsPath(repoPath, sshHost),
    content: buildKanvibeHookTargetsContent(currentContent, { url: hookServerUrl, taskId }),
  };
}

function parseRemoteJsonObject(content: string): Record<string, unknown> {
  if (!content) {
    return {};
  }

  try {
    return JSON.parse(content) as Record<string, unknown>;
  } catch {
    return {};
  }
}

interface RemoteTextFile {
  filePath: string;
  content: string;
  mode?: number;
}

async function writeRemoteTextFiles(files: RemoteTextFile[], sshHost: string): Promise<void> {
  const command = files.map(({ filePath, content, mode }) => {
    const encodedContent = Buffer.from(content, "utf-8").toString("base64");
    const parts = [
      `mkdir -p ${quoteShellArgument(path.posix.dirname(filePath))}`,
      `printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quoteShellArgument(filePath)}`,
    ];

    if (mode) {
      parts.push(`chmod ${mode.toString(8)} ${quoteShellArgument(filePath)}`);
    }

    return parts.join(" && ");
  }).join(" && ");

  await execGit(command, sshHost);
}

function upsertHookEntry(
  hooks: Record<string, unknown[]>,
  bucket: string,
  scriptName: string,
  entry: Record<string, unknown>,
) {
  const currentEntries = Array.isArray(hooks[bucket]) ? hooks[bucket] : [];
  hooks[bucket] = currentEntries.filter((value) => !JSON.stringify(value).includes(scriptName));
  hooks[bucket].push(entry);
}

function assertHookInstallResults(results: PromiseSettledResult<unknown>[], providers: string[]) {
  const failureIndex = results.findIndex((result) => result.status === "rejected");
  if (failureIndex === -1) {
    return;
  }

  const failure = results[failureIndex] as PromiseRejectedResult;
  console.error(`[hooks] ${providers[failureIndex]} install failed`, {
    provider: providers[failureIndex],
    error: failure.reason instanceof Error ? failure.reason.message : String(failure.reason),
  });
  throw failure.reason instanceof Error ? failure.reason : new Error("hooks 설정 실패");
}

type HookVerificationStatus = ClaudeHooksStatus | GeminiHooksStatus | CodexHooksStatus | OpenCodeHooksStatus;

interface HookVerificationFailure {
  provider: string;
  failedChecks?: string[];
  error?: unknown;
}

async function verifyHookInstallation(targetPath: string, taskId: string, sshHost?: string | null) {
  const failures = await logHookVerificationStatuses(targetPath, taskId, sshHost);
  if (failures.length > 0) {
    throw new Error(`hooks 검증 실패: ${formatHookVerificationFailures(failures)}`);
  }
}

async function verifyHookProviderInstallation(
  installer: HookProviderInstaller,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
) {
  const failure = await logHookProviderVerificationStatus(installer, targetPath, taskId, sshHost);
  if (failure) {
    throw new Error(`hooks 검증 실패: ${formatHookVerificationFailures([failure])}`);
  }
}

async function logHookVerificationStatuses(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<HookVerificationFailure[]> {
  const verifiers = [
    { provider: "Claude", verify: getClaudeHooksStatus },
    { provider: "Gemini", verify: getGeminiHooksStatus },
    { provider: "Codex", verify: getCodexHooksStatus },
    { provider: "OpenCode", verify: getOpenCodeHooksStatus },
  ] as const;

  const results = await Promise.allSettled(verifiers.map(({ verify }) => verify(targetPath, taskId, sshHost)));
  const failures: HookVerificationFailure[] = [];

  for (const [index, result] of results.entries()) {
    const provider = verifiers[index].provider;
    if (result.status === "fulfilled") {
      const failedChecks = logHookVerificationStatus(provider, result.value, targetPath, taskId, sshHost);
      const fatalFailedChecks = getFatalHookVerificationFailedChecks(provider, result.value, failedChecks);
      if (shouldFailHookVerification(result.value, failedChecks, fatalFailedChecks)) {
        failures.push({ provider, failedChecks: fatalFailedChecks });
      }
      continue;
    }

    console.warn(`[hooks] ${provider} verification unavailable`, {
      provider,
      targetPath,
      taskId,
      sshHost: sshHost ?? null,
      error: result.reason instanceof Error ? result.reason.message : String(result.reason),
    });
    failures.push({ provider, error: result.reason });
  }

  return failures;
}

async function logHookProviderVerificationStatus(
  installer: HookProviderInstaller,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<HookVerificationFailure | null> {
  try {
    const status = await installer.verify();
    const failedChecks = logHookVerificationStatus(installer.label, status, targetPath, taskId, sshHost);
    const fatalFailedChecks = getFatalHookVerificationFailedChecks(installer.label, status, failedChecks);
    return shouldFailHookVerification(status, failedChecks, fatalFailedChecks)
      ? { provider: installer.label, failedChecks: fatalFailedChecks }
      : null;
  } catch (error) {
    console.warn(`[hooks] ${installer.label} verification unavailable`, {
      provider: installer.label,
      targetPath,
      taskId,
      sshHost: sshHost ?? null,
      error: error instanceof Error ? error.message : String(error),
    });
    return { provider: installer.label, error };
  }
}

function logHookVerificationStatus(
  provider: string,
  status: HookVerificationStatus,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): string[] {
  const failedChecks = getHookVerificationFailedChecks(status);
  const payload = {
    provider,
    targetPath,
    taskId,
    sshHost: sshHost ?? null,
    installed: status.installed,
    failedChecks,
    boundTaskId: status.boundTaskId ?? null,
    configuredHookServerUrl: status.configuredHookServerUrl ?? null,
    expectedHookServerUrl: status.expectedHookServerUrl ?? null,
    registeredPluginUrls: "registeredPluginUrls" in status && Array.isArray(status.registeredPluginUrls)
      ? status.registeredPluginUrls
      : undefined,
  };

  if (status.installed) {
    console.log(`[hooks] ${provider} verification`, payload);
    return failedChecks;
  }

  console.warn(`[hooks] ${provider} verification`, payload);
  return failedChecks;
}

function getHookVerificationFailedChecks(status: HookVerificationStatus): string[] {
  return Object.entries(status)
    .filter(([key, value]) => key.startsWith("has") && value === false)
    .filter(([key]) => !NON_BLOCKING_HOOK_STATUS_CHECKS.has(key))
    .map(([key]) => key);
}

function getFatalHookVerificationFailedChecks(
  provider: string,
  status: HookVerificationStatus,
  failedChecks: string[],
): string[] {
  if (status.installed || provider !== "OpenCode") {
    return failedChecks;
  }

  return failedChecks.filter((check) => !OPEN_CODE_NON_BLOCKING_INSTALL_CHECKS.has(check));
}

function shouldFailHookVerification(
  status: HookVerificationStatus,
  failedChecks: string[],
  fatalFailedChecks: string[],
): boolean {
  if (status.installed) {
    return false;
  }

  return fatalFailedChecks.length > 0 || failedChecks.length === 0;
}

function formatHookVerificationFailures(failures: HookVerificationFailure[]): string {
  return failures.map(({ provider, failedChecks, error }) => {
    if (error) {
      return `${provider}(${error instanceof Error ? error.message : String(error)})`;
    }

    if (failedChecks && failedChecks.length > 0) {
      return `${provider}(${failedChecks.join(", ")})`;
    }

    return provider;
  }).join(", ");
}
