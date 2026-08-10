import { aggregateAiUsage } from "@/lib/aiUsage/aggregateAiUsage";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

export async function getAiUsageSnapshot(): Promise<AiUsageSnapshot> {
  return aggregateAiUsage();
}
