import type { ShellHookScriptsStatus } from "@/desktop/shared/contracts/lib-shellHookProvider";

export interface CodexHooksStatus extends Partial<ShellHookScriptsStatus> {
  installed: boolean;
  hasPromptHook: boolean;
  hasPermissionHook: boolean;
  hasPreToolHook: boolean;
  hasStopHook: boolean;
  hasHooksFile: boolean;
  hasHookEntries: boolean;
  hasConfigEntry: boolean;
}
