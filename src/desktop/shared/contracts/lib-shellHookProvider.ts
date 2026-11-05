import type { TextFileReadResult } from "@/desktop/shared/contracts/lib-hostFileAccess";
import type { HookTargetRegistrationStatus } from "@/desktop/shared/contracts/lib-hookTargetRegistration";
import type { ShellHookStatus } from "@/desktop/shared/contracts/lib-shellHookScript";

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
