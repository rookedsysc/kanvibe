import { readClaudeUsage } from "@/lib/aiUsage/readClaudeUsage";
import { readCodexUsage } from "@/lib/aiUsage/readCodexUsage";
import { readGeminiUsage } from "@/lib/aiUsage/readGeminiUsage";
import { createErrorUsage } from "@/lib/aiUsage/shared";
import type { AiUsageProvider, AiUsageProviderResult, AiUsageSnapshot } from "@/lib/aiUsage/types";

const USAGE_READERS: { provider: AiUsageProvider; read: () => Promise<AiUsageProviderResult> }[] = [
  { provider: "claude", read: readClaudeUsage },
  { provider: "codex", read: readCodexUsage },
  { provider: "gemini", read: readGeminiUsage },
];

/**
 * 한 provider의 실패가 나머지를 가리면 패널이 통째로 비어 버린다.
 * 각 결과를 따로 받아 실패한 자리에만 error를 채워 넣고, 순서는 항상 같게 유지해 화면이 흔들리지 않게 한다.
 */
export async function aggregateAiUsage(): Promise<AiUsageSnapshot> {
  const settledResults = await Promise.allSettled(USAGE_READERS.map(({ read }) => read()));
  const providers = settledResults.map((settled, index) => (
    settled.status === "fulfilled"
      ? settled.value
      : createErrorUsage(USAGE_READERS[index].provider, "fetch-failed")
  ));

  return {
    providers,
    fetchedAt: new Date().toISOString(),
  };
}
