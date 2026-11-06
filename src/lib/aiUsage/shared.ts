import type { AiUsageFailureReason } from "@/desktop/shared/contracts/lib-aiUsage-types";

export const SIGN_IN_FAILURE_REASONS: ReadonlySet<AiUsageFailureReason> = new Set([
  "missing-credentials", "expired-credentials", "gemini-cli-not-found",
]);
