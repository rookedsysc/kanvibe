import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

export function getAiUsageSnapshot(): Promise<AiUsageSnapshot> {
  return invokeDesktop("aiUsage", "getAiUsageSnapshot");
}

/** 마지막으로 저장된 조회 결과. 새 조회를 기다리는 동안 패널을 채우는 데 쓴다 */
export function getCachedAiUsageSnapshot(): Promise<AiUsageSnapshot | null> {
  return invokeDesktop("aiUsage", "getCachedAiUsageSnapshot");
}
