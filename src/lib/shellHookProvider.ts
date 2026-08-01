import { chmod, mkdir, writeFile } from "fs/promises";
import path from "path";
import { getKanvibeTargetsPath } from "@/lib/kanvibeProjectState";
import { readTextFiles, type TextFileReadResult } from "@/lib/hostFileAccess";
import {
  extractShellHookServerUrl,
  validateHookServerConfiguration,
  type HookServerValidation,
} from "@/lib/hookServerStatus";
import {
  verifyHookTargetRegistration,
  type HookTargetRegistrationStatus,
} from "@/lib/hookTargetRegistration";
import {
  buildShellKanvibeStatusUpdater,
  buildShellTaskIdResolver,
  getShellTaskIdBindingStatus,
  hasShellKanvibeBoundedNotifyTimeout,
  hasShellKanvibeParallelTargetFanout,
  hasShellKanvibeStatusJsonPersistence,
  hasShellKanvibeTargetFanout,
  type ShellHookStatus,
} from "@/lib/shellHookScript";

const HOOK_SCRIPT_FILE_MODE = 0o755;

/** 하나의 CLI 이벤트에 대응하는 KanVibe hook shell script 정의 */
export interface ShellHookScriptDefinition {
  fileName: string;
  eventLabel: string;
  description: string;
  status: ShellHookStatus;
  /** stdout에 JSON만 허용하는 런타임(Gemini CLI)을 위해 종료 직전 출력할 내용 */
  trailingOutput?: string;
  /** 스크립트 상단 주석에 덧붙일 런타임 제약 설명 */
  runtimeNote?: string;
}

export interface ShellHookProviderFile {
  filePath: string;
  content: string;
  mode?: number;
}

/** 모든 shell hook provider가 공유하는 설치 상태 지표 */
export interface ShellHookScriptsStatus extends HookTargetRegistrationStatus {
  hasTaskIdBinding: boolean;
  hasStatusMappings: boolean;
  hasStatusJsonPersistence: boolean;
  hasTargetFanout: boolean;
  hasBoundedNotifyTimeout: boolean;
  hasParallelTargetFanout: boolean;
  hasExpectedHookServerUrl: boolean;
  hasReachableHookServer: boolean;
  boundTaskId: string | null;
  configuredHookServerUrl: string | null;
  expectedHookServerUrl: string | null;
}

export interface ShellHookProviderState {
  /** 정의 순서대로의 스크립트 존재 여부와 내용 */
  scriptFiles: TextFileReadResult[];
  hasAllScripts: boolean;
  /** `extraFilePaths`로 요청한 파일 내용 */
  files: Map<string, TextFileReadResult>;
  status: ShellHookScriptsStatus;
}

/** hook shell script 한 개의 내용을 만든다 */
export function generateShellHookScript(
  agentLabel: string,
  definition: ShellHookScriptDefinition,
  kanvibeUrl: string,
  taskId: string,
): string {
  const headerComments = [
    `# KanVibe ${agentLabel} Hook: ${definition.eventLabel}`,
    `# ${definition.description}`,
    ...(definition.runtimeNote ? [`# ${definition.runtimeNote}`] : []),
  ].join("\n");
  const trailingLines = [
    ...(definition.trailingOutput ? [definition.trailingOutput] : []),
    "exit 0",
  ].join("\n");

  return `#!/bin/bash

${headerComments}

KANVIBE_URL="${kanvibeUrl}"
${buildShellTaskIdResolver(taskId)}
${buildShellKanvibeStatusUpdater(definition.status)}

${trailingLines}
`;
}

/** hook script 파일 목록을 만든다. 로컬 설치와 원격 설치가 같은 내용을 공유한다 */
export function buildShellHookScriptFiles(
  hooksDir: string,
  agentLabel: string,
  definitions: ShellHookScriptDefinition[],
  kanvibeUrl: string,
  taskId: string,
  sshHost?: string | null,
): ShellHookProviderFile[] {
  const pathModule = resolvePathModule(sshHost);

  return definitions.map((definition) => ({
    filePath: pathModule.join(hooksDir, definition.fileName),
    content: generateShellHookScript(agentLabel, definition, kanvibeUrl, taskId),
    mode: HOOK_SCRIPT_FILE_MODE,
  }));
}

/** 로컬 repo에 hook script를 기록하고 실행 권한을 부여한다 */
export async function writeLocalShellHookScripts(
  hooksDir: string,
  agentLabel: string,
  definitions: ShellHookScriptDefinition[],
  kanvibeUrl: string,
  taskId: string,
): Promise<void> {
  await mkdir(hooksDir, { recursive: true });

  const files = buildShellHookScriptFiles(hooksDir, agentLabel, definitions, kanvibeUrl, taskId);
  for (const file of files) {
    await writeFile(file.filePath, file.content, "utf-8");
    await chmod(file.filePath, HOOK_SCRIPT_FILE_MODE);
  }
}

/**
 * hook script와 provider별 설정 파일, `.kanvibe/targets.json`을 한 번에 읽어
 * 모든 shell hook provider가 공유하는 설치 상태를 계산한다.
 */
export async function readShellHookProviderState(options: {
  repoPath: string;
  hooksDir: string;
  definitions: ShellHookScriptDefinition[];
  extraFilePaths?: string[];
  taskId?: string;
  sshHost?: string | null;
}): Promise<ShellHookProviderState> {
  const { repoPath, hooksDir, definitions, extraFilePaths = [], taskId, sshHost } = options;
  const pathModule = resolvePathModule(sshHost);
  const scriptPaths = definitions.map((definition) => pathModule.join(hooksDir, definition.fileName));
  const targetsPath = getKanvibeTargetsPath(repoPath, sshHost);

  const files = await readTextFiles([...scriptPaths, ...extraFilePaths, targetsPath], sshHost);
  const scriptFiles = scriptPaths.map((scriptPath) => files.get(scriptPath) ?? emptyFile());
  const scriptContents = scriptFiles.map((file) => file.content);

  const hookServerValidation = await validateHookServerConfiguration(
    scriptContents.map(extractShellHookServerUrl),
    Boolean(taskId),
    sshHost,
  );

  return {
    scriptFiles,
    hasAllScripts: scriptFiles.every((file) => file.exists),
    files,
    status: buildShellHookScriptsStatus({
      definitions,
      scriptContents,
      targetsContent: (files.get(targetsPath) ?? emptyFile()).content,
      taskId,
      hookServerValidation,
    }),
  };
}

function buildShellHookScriptsStatus(options: {
  definitions: ShellHookScriptDefinition[];
  scriptContents: string[];
  targetsContent: string;
  taskId?: string;
  hookServerValidation: HookServerValidation;
}): ShellHookScriptsStatus {
  const { definitions, scriptContents, targetsContent, taskId, hookServerValidation } = options;
  const { boundTaskId, hasTaskIdBinding } = getShellTaskIdBindingStatus(scriptContents);

  return {
    hasTaskIdBinding,
    hasStatusMappings: definitions.every((definition, index) => (
      scriptContents[index]?.includes(buildStatusPayloadFragment(definition.status)) ?? false
    )),
    hasStatusJsonPersistence: scriptContents.every(hasShellKanvibeStatusJsonPersistence),
    hasTargetFanout: scriptContents.every(hasShellKanvibeTargetFanout),
    hasBoundedNotifyTimeout: scriptContents.every(hasShellKanvibeBoundedNotifyTimeout),
    hasParallelTargetFanout: scriptContents.every(hasShellKanvibeParallelTargetFanout),
    hasExpectedHookServerUrl: hookServerValidation.hasExpectedHookServerUrl,
    hasReachableHookServer: hookServerValidation.hasReachableHookServer,
    boundTaskId,
    configuredHookServerUrl: hookServerValidation.configuredHookServerUrl,
    expectedHookServerUrl: hookServerValidation.expectedHookServerUrl,
    ...verifyHookTargetRegistration(targetsContent, taskId, hookServerValidation.expectedHookServerUrl),
  };
}

/** hook script가 자신의 상태를 실제로 payload에 담고 있는지 확인할 문자열 */
function buildStatusPayloadFragment(status: ShellHookStatus): string {
  return `\\"status\\": \\"${status}\\"`;
}

/**
 * hook script와 설정이 모두 정상인지 판정한다. provider별 추가 조건은 호출부가 결합한다.
 * 알림 대상은 script에 박힌 URL이 아니라 targets.json 등록 여부로 판정하므로,
 * 다른 client가 script의 fallback URL/TASK_ID를 덮어써도 설치 상태가 깨지지 않는다.
 */
export function isShellHookProviderInstalled(
  state: ShellHookProviderState,
  providerChecks: boolean[],
): boolean {
  const { status } = state;

  return state.hasAllScripts
    && providerChecks.every(Boolean)
    && status.hasTaskIdBinding
    && status.hasRegisteredHookTarget
    && status.hasStatusMappings
    && status.hasStatusJsonPersistence
    && status.hasTargetFanout
    && status.hasBoundedNotifyTimeout
    && status.hasParallelTargetFanout;
}

export function resolvePathModule(sshHost?: string | null): typeof path.posix | typeof path {
  return sshHost ? path.posix : path;
}

function emptyFile(): TextFileReadResult {
  return { exists: false, content: "" };
}
