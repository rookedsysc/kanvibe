import type { HookTargetRegistrationStatus } from "@/desktop/shared/contracts/lib-hookTargetRegistration";

export interface OpenCodeHooksStatus extends Partial<HookTargetRegistrationStatus> {
  installed: boolean;
  hasPlugin: boolean;
  hasRegisteredPlugin?: boolean;
  hasDuplicateKanvibePlugins?: boolean;
  hasTaskIdBinding?: boolean;
  hasStatusEndpoint?: boolean;
  hasEventMappings?: boolean;
  hasStatusJsonPersistence?: boolean;
  hasBoundedNotifyTimeout?: boolean;
  hasTargetFanout?: boolean;
  hasParallelTargetFanout?: boolean;
  hasMainSessionGuard?: boolean;
  hasDuplicateProgressGuard?: boolean;
  hasExpectedHookServerUrl?: boolean;
  hasReachableHookServer?: boolean;
  boundTaskId?: string | null;
  targetPath?: string | null;
  pluginPath?: string | null;
  registeredPluginUrls?: string[];
  configuredHookServerUrl?: string | null;
  expectedHookServerUrl?: string | null;
}
