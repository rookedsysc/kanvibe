import { invokeDesktop } from "@/desktop/renderer/ipc";
import type {
  ReleaseUpdate,
  ReleaseUpdateCheckResult,
} from "@/desktop/shared/contracts/desktop-main-services-releaseUpdateService";

export type {
  ReleaseUpdate,
  ReleaseUpdateCheckResult,
};

export function checkForReleaseUpdate(): Promise<ReleaseUpdateCheckResult> {
  return invokeDesktop("releaseUpdates", "checkForReleaseUpdate");
}

export function claimReleaseUpdateVersion(version: string): Promise<boolean> {
  return invokeDesktop("releaseUpdates", "claimReleaseUpdateVersion", version);
}
