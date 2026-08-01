import {
  buildClaudeHookFiles,
  getClaudeHookInstallInputPaths,
  getClaudeHooksStatus,
  setupClaudeHooks,
  type ClaudeHooksStatus,
} from "@/lib/claudeHooksSetup";
import {
  buildCodexHookFiles,
  getCodexHookInstallInputPaths,
  getCodexHooksStatus,
  setupCodexHooks,
  type CodexHooksStatus,
} from "@/lib/codexHooksSetup";
import {
  buildGeminiHookFiles,
  getGeminiHookInstallInputPaths,
  getGeminiHooksStatus,
  setupGeminiHooks,
  type GeminiHooksStatus,
} from "@/lib/geminiHooksSetup";
import {
  buildOpenCodeHookFiles,
  getOpenCodeHookInstallInputPaths,
  getOpenCodeHooksStatus,
  setupOpenCodeHooks,
  type OpenCodeHooksStatus,
} from "@/lib/openCodeHooksSetup";
import type { TextFileReadResult } from "@/lib/hostFileAccess";
import type { ShellHookProviderFile } from "@/lib/shellHookProvider";

export type KanvibeHookProvider = "claude" | "gemini" | "codex" | "openCode";

export type KanvibeHookStatus =
  | ClaudeHooksStatus
  | GeminiHooksStatus
  | CodexHooksStatus
  | OpenCodeHooksStatus;

/**
 * AI CLI 하나의 hook 설치 규약.
 * 로컬/원격, 단일 provider/일괄 설치가 모두 같은 산출물 생성 규칙을 공유하도록 묶는다.
 */
export interface KanvibeHookProviderModule {
  provider: KanvibeHookProvider;
  label: string;
  /** 산출물을 만들기 전에 읽어야 하는 기존 설정 파일 경로 */
  getInstallInputPaths: (repoPath: string, sshHost?: string | null) => string[];
  buildFiles: (
    repoPath: string,
    taskId: string,
    hookServerUrl: string,
    existingFiles: Map<string, TextFileReadResult>,
    sshHost?: string | null,
  ) => ShellHookProviderFile[];
  install: (
    repoPath: string,
    taskId: string,
    hookServerUrl: string,
    sshHost?: string | null,
  ) => Promise<void>;
  getStatus: (
    repoPath: string,
    taskId?: string,
    sshHost?: string | null,
  ) => Promise<KanvibeHookStatus>;
}

export const KANVIBE_HOOK_PROVIDER_MODULES: Record<KanvibeHookProvider, KanvibeHookProviderModule> = {
  claude: {
    provider: "claude",
    label: "Claude",
    getInstallInputPaths: getClaudeHookInstallInputPaths,
    buildFiles: (repoPath, taskId, hookServerUrl, existingFiles, sshHost) => buildClaudeHookFiles(
      repoPath,
      taskId,
      hookServerUrl,
      readExistingContent(existingFiles, getClaudeHookInstallInputPaths(repoPath, sshHost)[0]),
      sshHost,
    ),
    install: setupClaudeHooks,
    getStatus: getClaudeHooksStatus,
  },
  gemini: {
    provider: "gemini",
    label: "Gemini",
    getInstallInputPaths: getGeminiHookInstallInputPaths,
    buildFiles: (repoPath, taskId, hookServerUrl, existingFiles, sshHost) => buildGeminiHookFiles(
      repoPath,
      taskId,
      hookServerUrl,
      readExistingContent(existingFiles, getGeminiHookInstallInputPaths(repoPath, sshHost)[0]),
      sshHost,
    ),
    install: setupGeminiHooks,
    getStatus: getGeminiHooksStatus,
  },
  codex: {
    provider: "codex",
    label: "Codex",
    getInstallInputPaths: getCodexHookInstallInputPaths,
    buildFiles: (repoPath, taskId, hookServerUrl, existingFiles, sshHost) => {
      const [configPath, hooksPath] = getCodexHookInstallInputPaths(repoPath, sshHost);
      return buildCodexHookFiles(
        repoPath,
        taskId,
        hookServerUrl,
        readExistingContent(existingFiles, configPath),
        readExistingContent(existingFiles, hooksPath),
        sshHost,
      );
    },
    install: setupCodexHooks,
    getStatus: getCodexHooksStatus,
  },
  openCode: {
    provider: "openCode",
    label: "OpenCode",
    getInstallInputPaths: getOpenCodeHookInstallInputPaths,
    buildFiles: (repoPath, taskId, hookServerUrl, _existingFiles, sshHost) => buildOpenCodeHookFiles(
      repoPath,
      taskId,
      hookServerUrl,
      sshHost,
    ),
    install: setupOpenCodeHooks,
    getStatus: getOpenCodeHooksStatus,
  },
};

export function getKanvibeHookProviderModules(): KanvibeHookProviderModule[] {
  return Object.values(KANVIBE_HOOK_PROVIDER_MODULES);
}

function readExistingContent(files: Map<string, TextFileReadResult>, filePath: string | undefined): string {
  return filePath ? files.get(filePath)?.content ?? "" : "";
}
