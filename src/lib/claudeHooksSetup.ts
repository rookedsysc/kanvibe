import { registerKanvibeHookTarget } from "@/lib/hookTargetRegistration";
import { readTextFiles } from "@/lib/hostFileAccess";
import { writeHookProviderFiles } from "@/lib/hookFileWriter";
import {
  buildCommandHookEntry,
  buildMatcherCommandHookEntry,
  ensureJsonHookBuckets,
  hasCommandHookEntry,
  hasMatcherCommandHookEntry,
  parseJsonHookSettings,
  serializeJsonHookSettings,
  upsertJsonHookEntries,
} from "@/lib/jsonHookSettings";
import {
  buildShellHookScriptFiles,
  generateShellHookScript,
  isShellHookProviderInstalled,
  readShellHookProviderState,
  resolvePathModule,
  type ShellHookProviderFile,
  type ShellHookScriptDefinition,
  type ShellHookScriptsStatus,
} from "@/lib/shellHookProvider";

const AGENT_LABEL = "Claude Code";
const CONFIG_DIR_NAME = ".claude";
const SETTINGS_FILE_NAME = "settings.json";
const HOOK_TIMEOUT_SECONDS = 10;
const ASK_USER_QUESTION_MATCHER = "AskUserQuestion";

export const PROMPT_HOOK_SCRIPT_NAME = "kanvibe-prompt-hook.sh";
export const STOP_HOOK_SCRIPT_NAME = "kanvibe-stop-hook.sh";
export const QUESTION_HOOK_SCRIPT_NAME = "kanvibe-question-hook.sh";

const CLAUDE_PROMPT_COMMAND = `"$CLAUDE_PROJECT_DIR"/${CONFIG_DIR_NAME}/hooks/${PROMPT_HOOK_SCRIPT_NAME}`;
const CLAUDE_STOP_COMMAND = `"$CLAUDE_PROJECT_DIR"/${CONFIG_DIR_NAME}/hooks/${STOP_HOOK_SCRIPT_NAME}`;
const CLAUDE_QUESTION_COMMAND = `"$CLAUDE_PROJECT_DIR"/${CONFIG_DIR_NAME}/hooks/${QUESTION_HOOK_SCRIPT_NAME}`;

const CLAUDE_HOOK_SCRIPTS: ShellHookScriptDefinition[] = [
  {
    fileName: PROMPT_HOOK_SCRIPT_NAME,
    eventLabel: "UserPromptSubmit",
    description: "사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.",
    status: "progress",
  },
  {
    fileName: STOP_HOOK_SCRIPT_NAME,
    eventLabel: "Stop",
    description: "AI 응답이 완료되면 현재 task를 REVIEW로 변경한다.",
    status: "review",
  },
  {
    fileName: QUESTION_HOOK_SCRIPT_NAME,
    eventLabel: `PreToolUse (${ASK_USER_QUESTION_MATCHER})`,
    description: "Claude가 사용자에게 질문할 때 현재 task를 PENDING으로 변경한다.",
    status: "pending",
  },
];

export interface ClaudeHooksStatus extends Partial<ShellHookScriptsStatus> {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasQuestionHook: boolean;
  hasSettingsEntry: boolean;
}

/** UserPromptSubmit hook bash 스크립트를 생성한다 */
export function generatePromptHookScript(kanvibeUrl: string, taskId: string): string {
  return generateShellHookScript(AGENT_LABEL, CLAUDE_HOOK_SCRIPTS[0], kanvibeUrl, taskId);
}

/** Stop hook bash 스크립트를 생성한다 */
export function generateStopHookScript(kanvibeUrl: string, taskId: string): string {
  return generateShellHookScript(AGENT_LABEL, CLAUDE_HOOK_SCRIPTS[1], kanvibeUrl, taskId);
}

/** PreToolUse(AskUserQuestion) hook bash 스크립트를 생성한다 */
export function generateQuestionHookScript(kanvibeUrl: string, taskId: string): string {
  return generateShellHookScript(AGENT_LABEL, CLAUDE_HOOK_SCRIPTS[2], kanvibeUrl, taskId);
}

/** 설치 파일을 만들기 전에 읽어야 하는 기존 설정 파일 경로 */
export function getClaudeHookInstallInputPaths(repoPath: string, sshHost?: string | null): string[] {
  return [getClaudeSettingsPath(repoPath, sshHost)];
}

/**
 * Claude Code hooks 설치 산출물을 만든다.
 * 기존 settings.json이 있으면 kanvibe hooks 항목만 갱신하고 나머지는 보존한다.
 */
export function buildClaudeHookFiles(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  settingsContent: string,
  sshHost?: string | null,
): ShellHookProviderFile[] {
  const pathModule = resolvePathModule(sshHost);
  const hooksDir = pathModule.join(repoPath, CONFIG_DIR_NAME, "hooks");
  const settings = parseJsonHookSettings(settingsContent);
  const hooks = ensureJsonHookBuckets(settings);

  hooks.UserPromptSubmit = upsertJsonHookEntries(
    hooks.UserPromptSubmit,
    PROMPT_HOOK_SCRIPT_NAME,
    buildCommandHookEntry(CLAUDE_PROMPT_COMMAND, HOOK_TIMEOUT_SECONDS),
  );
  hooks.PreToolUse = upsertJsonHookEntries(
    hooks.PreToolUse,
    QUESTION_HOOK_SCRIPT_NAME,
    buildMatcherCommandHookEntry(ASK_USER_QUESTION_MATCHER, CLAUDE_QUESTION_COMMAND, HOOK_TIMEOUT_SECONDS),
  );
  hooks.PostToolUse = upsertJsonHookEntries(
    hooks.PostToolUse,
    PROMPT_HOOK_SCRIPT_NAME,
    buildMatcherCommandHookEntry(ASK_USER_QUESTION_MATCHER, CLAUDE_PROMPT_COMMAND, HOOK_TIMEOUT_SECONDS),
  );
  hooks.Stop = upsertJsonHookEntries(
    hooks.Stop,
    STOP_HOOK_SCRIPT_NAME,
    buildCommandHookEntry(CLAUDE_STOP_COMMAND, HOOK_TIMEOUT_SECONDS),
  );

  return [
    ...buildShellHookScriptFiles(hooksDir, AGENT_LABEL, CLAUDE_HOOK_SCRIPTS, kanvibeUrl, taskId, sshHost),
    {
      filePath: getClaudeSettingsPath(repoPath, sshHost),
      content: serializeJsonHookSettings(settings),
    },
  ];
}

/** 지정된 repo에 Claude Code hooks를 설정한다. 로컬과 원격 repo 모두 같은 산출물을 기록한다 */
export async function setupClaudeHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  sshHost?: string | null,
): Promise<void> {
  const settingsPath = getClaudeSettingsPath(repoPath, sshHost);
  const existingFiles = await readTextFiles([settingsPath], sshHost);

  await writeHookProviderFiles(
    buildClaudeHookFiles(repoPath, taskId, kanvibeUrl, existingFiles.get(settingsPath)?.content ?? "", sshHost),
    sshHost,
  );

  await registerKanvibeHookTarget(repoPath, taskId, kanvibeUrl, sshHost);
}

/** 지정된 repo의 Claude Code hooks 설치 상태를 확인한다 */
export async function getClaudeHooksStatus(
  repoPath: string,
  taskId?: string,
  sshHost?: string | null,
): Promise<ClaudeHooksStatus> {
  const settingsPath = getClaudeSettingsPath(repoPath, sshHost);
  const state = await readShellHookProviderState({
    repoPath,
    hooksDir: resolvePathModule(sshHost).join(repoPath, CONFIG_DIR_NAME, "hooks"),
    definitions: CLAUDE_HOOK_SCRIPTS,
    extraFilePaths: [settingsPath],
    taskId,
    sshHost,
  });
  const [promptScript, stopScript, questionScript] = state.scriptFiles;
  const hasSettingsEntry = hasClaudeSettingsEntry(state.files.get(settingsPath)?.content ?? "");

  return {
    installed: isShellHookProviderInstalled(state, [hasSettingsEntry]),
    hasPromptHook: promptScript.exists,
    hasStopHook: stopScript.exists,
    hasQuestionHook: questionScript.exists,
    hasSettingsEntry,
    ...state.status,
  };
}

function hasClaudeSettingsEntry(settingsContent: string): boolean {
  const hooks = parseJsonHookSettings(settingsContent).hooks;
  if (!hooks) {
    return false;
  }

  return hasCommandHookEntry(hooks.UserPromptSubmit || [], CLAUDE_PROMPT_COMMAND)
    && hasCommandHookEntry(hooks.Stop || [], CLAUDE_STOP_COMMAND)
    && hasMatcherCommandHookEntry(hooks.PreToolUse || [], ASK_USER_QUESTION_MATCHER, CLAUDE_QUESTION_COMMAND)
    && hasMatcherCommandHookEntry(hooks.PostToolUse || [], ASK_USER_QUESTION_MATCHER, CLAUDE_PROMPT_COMMAND);
}

function getClaudeSettingsPath(repoPath: string, sshHost?: string | null): string {
  return resolvePathModule(sshHost).join(repoPath, CONFIG_DIR_NAME, SETTINGS_FILE_NAME);
}
