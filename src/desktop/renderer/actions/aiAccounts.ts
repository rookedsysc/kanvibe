import { invokeDesktop } from "@/desktop/renderer/ipc";
import type {
  AiAccountMutationResult,
  AiAccountSummary,
} from "@/desktop/main/services/aiAccountService";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

export function listAiAccounts(): Promise<AiAccountSummary[]> {
  return invokeDesktop("aiAccounts", "listAiAccounts");
}

export function addAiAccount(
  provider: AiUsageProvider,
  accountName: string,
): Promise<AiAccountMutationResult & { accountRoot?: string }> {
  return invokeDesktop("aiAccounts", "addAiAccount", provider, accountName);
}

export function removeAiAccount(
  provider: AiUsageProvider,
  accountRoot: string,
): Promise<AiAccountMutationResult> {
  return invokeDesktop("aiAccounts", "removeAiAccount", provider, accountRoot);
}
