import type { AiUsageProvider } from "@/desktop/shared/contracts/lib-aiUsage-types";

export interface AiAccountSummary {
  provider: AiUsageProvider;
  /** CLI에 계정 위치를 알릴 때 넘기는 루트이자 이 목록에서 계정을 가리키는 식별자 */
  accountRoot: string;
  /** 사용자가 붙인 계정 이름. 홈의 기본 계정은 이름이 없다 */
  accountName: string | null;
  label: string;
  isLoggedIn: boolean;
  planName: string | null;
  /** KanVibe가 만든 계정만 앱에서 지울 수 있다 */
  isRemovable: boolean;
}

export type AiAccountMutationResult =
  | { outcome: "ok" }
  | { outcome: "invalid-name" }
  | { outcome: "already-exists" }
  | { outcome: "manual-logout-required" };
