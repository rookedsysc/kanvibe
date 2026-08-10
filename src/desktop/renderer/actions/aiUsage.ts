import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

export function getAiUsageSnapshot(): Promise<AiUsageSnapshot> {
  return invokeDesktop("aiUsage", "getAiUsageSnapshot");
}
