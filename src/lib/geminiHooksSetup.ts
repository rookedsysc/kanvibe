import { registerKanvibeHookTarget } from "@/lib/hookTargetRegistration";
import { readTextFiles } from "@/lib/hostFileAccess";
import { writeHookProviderFiles } from "@/lib/hookFileWriter";
import {
  buildMatcherCommandHookEntry,
  ensureJsonHookBuckets,
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

/**
 * Gemini CLI hooks는 stdout에 반드시 JSON만 출력해야 한다.
 * curl 결과는 /dev/null로 보내고, 마지막에 '{}' JSON을 출력한다.
 */
const AGENT_LABEL = "Gemini CLI";
const CONFIG_DIR_NAME = ".gemini";
const SETTINGS_FILE_NAME = "settings.json";
const HOOK_TIMEOUT_MS = 10000;
const ALL_EVENTS_MATCHER = "*";
const JSON_ONLY_STDOUT_NOTE = "Gemini CLI hooks는 stdout에 JSON만 출력해야 한다.";
const EMPTY_JSON_OUTPUT = "echo '{}'";

export const PROMPT_HOOK_SCRIPT_NAME = "kanvibe-prompt-hook.sh";
export const STOP_HOOK_SCRIPT_NAME = "kanvibe-stop-hook.sh";

const GEMINI_PROMPT_COMMAND = `"$GEMINI_PROJECT_DIR"/${CONFIG_DIR_NAME}/hooks/${PROMPT_HOOK_SCRIPT_NAME}`;
const GEMINI_STOP_COMMAND = `"$GEMINI_PROJECT_DIR"/${CONFIG_DIR_NAME}/hooks/${STOP_HOOK_SCRIPT_NAME}`;

const GEMINI_HOOK_SCRIPTS: ShellHookScriptDefinition[] = [
  {
    fileName: PROMPT_HOOK_SCRIPT_NAME,
    eventLabel: "BeforeAgent",
    description: "사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.",
    status: "progress",
    runtimeNote: JSON_ONLY_STDOUT_NOTE,
    trailingOutput: EMPTY_JSON_OUTPUT,
  },
  {
    fileName: STOP_HOOK_SCRIPT_NAME,
    eventLabel: "AfterAgent",
    description: "AI 응답이 완료되면 현재 task를 REVIEW로 변경한다.",
    status: "review",
    runtimeNote: JSON_ONLY_STDOUT_NOTE,
    trailingOutput: EMPTY_JSON_OUTPUT,
  },
];

export interface GeminiHooksStatus extends Partial<ShellHookScriptsStatus> {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasSettingsEntry: boolean;
}

/** BeforeAgent hook bash 스크립트를 생성한다 */
export function generatePromptHookScript(kanvibeUrl: string, taskId: string): string {
  return generateShellHookScript(AGENT_LABEL, GEMINI_HOOK_SCRIPTS[0], kanvibeUrl, taskId);
}

/** AfterAgent hook bash 스크립트를 생성한다 */
export function generateStopHookScript(kanvibeUrl: string, taskId: string): string {
  return generateShellHookScript(AGENT_LABEL, GEMINI_HOOK_SCRIPTS[1], kanvibeUrl, taskId);
}

/** 설치 파일을 만들기 전에 읽어야 하는 기존 설정 파일 경로 */
export function getGeminiHookInstallInputPaths(repoPath: string, sshHost?: string | null): string[] {
  return [getGeminiSettingsPath(repoPath, sshHost)];
}

/**
 * Gemini CLI hooks 설치 산출물을 만든다.
 * 기존 settings.json이 있으면 kanvibe hooks 항목만 갱신하고 나머지는 보존한다.
 */
export function buildGeminiHookFiles(
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

  hooks.BeforeAgent = upsertJsonHookEntries(
    hooks.BeforeAgent,
    PROMPT_HOOK_SCRIPT_NAME,
    buildMatcherCommandHookEntry(ALL_EVENTS_MATCHER, GEMINI_PROMPT_COMMAND, HOOK_TIMEOUT_MS),
  );
  hooks.AfterAgent = upsertJsonHookEntries(
    hooks.AfterAgent,
    STOP_HOOK_SCRIPT_NAME,
    buildMatcherCommandHookEntry(ALL_EVENTS_MATCHER, GEMINI_STOP_COMMAND, HOOK_TIMEOUT_MS),
  );

  return [
    ...buildShellHookScriptFiles(hooksDir, AGENT_LABEL, GEMINI_HOOK_SCRIPTS, kanvibeUrl, taskId, sshHost),
    {
      filePath: getGeminiSettingsPath(repoPath, sshHost),
      content: serializeJsonHookSettings(settings),
    },
  ];
}

/** 지정된 repo에 Gemini CLI hooks를 설정한다. 로컬과 원격 repo 모두 같은 산출물을 기록한다 */
export async function setupGeminiHooks(
  repoPath: string,
  taskId: string,
  kanvibeUrl: string,
  sshHost?: string | null,
): Promise<void> {
  const settingsPath = getGeminiSettingsPath(repoPath, sshHost);
  const existingFiles = await readTextFiles([settingsPath], sshHost);

  await writeHookProviderFiles(
    buildGeminiHookFiles(repoPath, taskId, kanvibeUrl, existingFiles.get(settingsPath)?.content ?? "", sshHost),
    sshHost,
  );

  await registerKanvibeHookTarget(repoPath, taskId, kanvibeUrl, sshHost);
}

/** 지정된 repo의 Gemini CLI hooks 설치 상태를 확인한다 */
export async function getGeminiHooksStatus(
  repoPath: string,
  taskId?: string,
  sshHost?: string | null,
): Promise<GeminiHooksStatus> {
  const settingsPath = getGeminiSettingsPath(repoPath, sshHost);
  const state = await readShellHookProviderState({
    repoPath,
    hooksDir: resolvePathModule(sshHost).join(repoPath, CONFIG_DIR_NAME, "hooks"),
    definitions: GEMINI_HOOK_SCRIPTS,
    extraFilePaths: [settingsPath],
    taskId,
    sshHost,
  });
  const [promptScript, stopScript] = state.scriptFiles;
  const hasSettingsEntry = hasGeminiSettingsEntry(state.files.get(settingsPath)?.content ?? "");

  return {
    installed: isShellHookProviderInstalled(state, [hasSettingsEntry]),
    hasPromptHook: promptScript.exists,
    hasStopHook: stopScript.exists,
    hasSettingsEntry,
    ...state.status,
  };
}

function hasGeminiSettingsEntry(settingsContent: string): boolean {
  const hooks = parseJsonHookSettings(settingsContent).hooks;
  if (!hooks) {
    return false;
  }

  return hasMatcherCommandHookEntry(hooks.BeforeAgent || [], ALL_EVENTS_MATCHER, GEMINI_PROMPT_COMMAND)
    && hasMatcherCommandHookEntry(hooks.AfterAgent || [], ALL_EVENTS_MATCHER, GEMINI_STOP_COMMAND);
}

function getGeminiSettingsPath(repoPath: string, sshHost?: string | null): string {
  return resolvePathModule(sshHost).join(repoPath, CONFIG_DIR_NAME, SETTINGS_FILE_NAME);
}
