import type { ShellHookScriptsStatus } from "@/desktop/shared/contracts/lib-shellHookProvider";

export interface GeminiHooksStatus extends Partial<ShellHookScriptsStatus> {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasSettingsEntry: boolean;
}
