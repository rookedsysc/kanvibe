import path from "node:path";
import { execGit } from "@/lib/gitOperations";
import { setupClaudeHooks, generatePromptHookScript as generateClaudePromptHookScript, generateStopHookScript as generateClaudeStopHookScript, generateQuestionHookScript as generateClaudeQuestionHookScript } from "@/lib/claudeHooksSetup";
import { setupGeminiHooks, generatePromptHookScript as generateGeminiPromptHookScript, generateStopHookScript as generateGeminiStopHookScript } from "@/lib/geminiHooksSetup";
import { setupCodexHooks, generateNotifyHookScript, HOOK_SCRIPT_NAME, CONFIG_FILE_NAME } from "@/lib/codexHooksSetup";
import { setupOpenCodeHooks, generatePluginScript, PLUGIN_DIR_NAME, PLUGIN_FILE_NAME } from "@/lib/openCodeHooksSetup";
import { getHookServerToken, getHookServerUrl } from "@/lib/hookEndpoint";

export async function installKanvibeHooks(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);
  const hookServerToken = getHookServerToken();

  if (!sshHost) {
    const results = await Promise.allSettled([
      setupClaudeHooks(targetPath, taskId, hookServerUrl, hookServerToken),
      setupGeminiHooks(targetPath, taskId, hookServerUrl, hookServerToken),
      setupCodexHooks(targetPath, taskId, hookServerUrl, hookServerToken),
      setupOpenCodeHooks(targetPath, taskId, hookServerUrl, hookServerToken),
    ]);
    assertHookInstallResults(results);
    return;
  }

  const remoteInstallers = [
    () => setupRemoteClaudeHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost),
    () => setupRemoteGeminiHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost),
    () => setupRemoteCodexHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost),
    () => setupRemoteOpenCodeHooks(targetPath, taskId, hookServerUrl, hookServerToken, sshHost),
  ];

  for (const installRemoteHooks of remoteInstallers) {
    await installRemoteHooks();
  }
}

async function setupRemoteClaudeHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const claudeDir = path.posix.join(repoPath, ".claude");
  const hooksDir = path.posix.join(claudeDir, "hooks");
  const settingsPath = path.posix.join(claudeDir, "settings.json");
  await execGit(`mkdir -p "${hooksDir}"`, sshHost);

  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"), generateClaudePromptHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-stop-hook.sh"), generateClaudeStopHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-question-hook.sh"), generateClaudeQuestionHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);

  const settings = await readRemoteJsonFile(settingsPath, sshHost);
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

  await writeRemoteTextFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", sshHost);
}

async function setupRemoteGeminiHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const geminiDir = path.posix.join(repoPath, ".gemini");
  const hooksDir = path.posix.join(geminiDir, "hooks");
  const settingsPath = path.posix.join(geminiDir, "settings.json");
  await execGit(`mkdir -p "${hooksDir}"`, sshHost);

  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-prompt-hook.sh"), generateGeminiPromptHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);
  await writeRemoteTextFile(path.posix.join(hooksDir, "kanvibe-stop-hook.sh"), generateGeminiStopHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);

  const settings = await readRemoteJsonFile(settingsPath, sshHost);
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

  await writeRemoteTextFile(settingsPath, JSON.stringify(settings, null, 2) + "\n", sshHost);
}

async function setupRemoteCodexHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const codexDir = path.posix.join(repoPath, ".codex");
  const hooksDir = path.posix.join(codexDir, "hooks");
  const configPath = path.posix.join(codexDir, CONFIG_FILE_NAME);
  await execGit(`mkdir -p "${hooksDir}"`, sshHost);

  await writeRemoteTextFile(path.posix.join(hooksDir, HOOK_SCRIPT_NAME), generateNotifyHookScript(hookServerUrl, taskId, hookServerToken), sshHost, 0o755);

  const configContent = await readRemoteTextFile(configPath, sshHost);
  const notifyLine = `notify = [".codex/hooks/${HOOK_SCRIPT_NAME}"]\n`;
  const nextConfig = configContent.includes(HOOK_SCRIPT_NAME)
    ? configContent.replace(/^notify\s*=.*$/m, `notify = [".codex/hooks/${HOOK_SCRIPT_NAME}"]`)
    : configContent.trim().length === 0
      ? notifyLine
      : `${configContent.trimEnd()}\n${notifyLine}`;
  await writeRemoteTextFile(configPath, nextConfig, sshHost);
}

async function setupRemoteOpenCodeHooks(repoPath: string, taskId: string, hookServerUrl: string, hookServerToken: string, sshHost: string) {
  const pluginDir = path.posix.join(repoPath, ".opencode", PLUGIN_DIR_NAME);
  await execGit(`mkdir -p "${pluginDir}"`, sshHost);
  await writeRemoteTextFile(
    path.posix.join(pluginDir, PLUGIN_FILE_NAME),
    generatePluginScript(hookServerUrl, taskId, hookServerToken),
    sshHost,
  );
}

async function readRemoteJsonFile(filePath: string, sshHost: string): Promise<Record<string, unknown>> {
  const content = await readRemoteTextFile(filePath, sshHost);
  if (!content) {
    return {};
  }

  try {
    return JSON.parse(content) as Record<string, unknown>;
  } catch {
    return {};
  }
}

async function readRemoteTextFile(filePath: string, sshHost: string): Promise<string> {
  try {
    return await execGit(`test -f "${filePath}" && cat "${filePath}" || true`, sshHost);
  } catch {
    return "";
  }
}

async function writeRemoteTextFile(filePath: string, content: string, sshHost: string, mode?: number): Promise<void> {
  const encodedContent = Buffer.from(content, "utf-8").toString("base64");
  const chmodCommand = mode ? ` && chmod ${mode.toString(8)} "${filePath}"` : "";
  await execGit(
    `mkdir -p "${path.posix.dirname(filePath)}" && printf '%s' '${encodedContent}' | (base64 -d 2>/dev/null || base64 -D) > "${filePath}"${chmodCommand}`,
    sshHost,
  );
}

function upsertHookEntry(
  hooks: Record<string, unknown[]>,
  bucket: string,
  scriptName: string,
  entry: Record<string, unknown>,
) {
  if (!hooks[bucket]) {
    hooks[bucket] = [];
  }

  const hasEntry = hooks[bucket].some((value) => JSON.stringify(value).includes(scriptName));
  if (!hasEntry) {
    hooks[bucket].push(entry);
  }
}

function assertHookInstallResults(results: PromiseSettledResult<unknown>[]) {
  const failure = results.find((result): result is PromiseRejectedResult => result.status === "rejected");
  if (failure) {
    throw failure.reason instanceof Error ? failure.reason : new Error("hooks 설정 실패");
  }
}
