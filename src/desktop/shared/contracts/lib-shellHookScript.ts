

export type ShellHookStatus = "progress" | "pending" | "review" | "done";

export interface ShellTaskIdBindingStatus {
  hasTaskIdBinding: boolean;
  boundTaskId: string | null;
}
