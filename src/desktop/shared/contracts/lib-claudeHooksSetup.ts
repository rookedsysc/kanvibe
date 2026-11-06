import type { ShellHookScriptsStatus } from "@/desktop/shared/contracts/lib-shellHookProvider";

export interface ClaudeHooksStatus extends Partial<ShellHookScriptsStatus> {
  installed: boolean;
  hasPromptHook: boolean;
  hasStopHook: boolean;
  hasQuestionHook: boolean;
  hasSettingsEntry: boolean;
}
