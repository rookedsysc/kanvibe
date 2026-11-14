import { invokeDesktop } from "@/desktop/renderer/ipc";
import type {
  HerdrCreatePlanRequest,
  HerdrExecutePlanRequest,
  HerdrSessionOperation,
  HerdrSessionPlan,
  HerdrSessionResult,
  HerdrSessionScan,
} from "@/desktop/shared/herdrSessions";

export function scanSessions(request: { sshHost: string | null }) {
  return invokeDesktop<HerdrSessionResult<HerdrSessionScan>>("herdrSession", "scanSessions", request);
}

export function createPlan(request: HerdrCreatePlanRequest) {
  return invokeDesktop<HerdrSessionResult<HerdrSessionPlan>>("herdrSession", "createPlan", request);
}

export function executePlan(request: HerdrExecutePlanRequest) {
  return invokeDesktop<HerdrSessionResult<HerdrSessionOperation>>("herdrSession", "executePlan", request);
}

export function getOperation(request: { operationId: string }) {
  return invokeDesktop<HerdrSessionResult<HerdrSessionOperation>>("herdrSession", "getOperation", request);
}

export function listOperations() {
  return invokeDesktop<HerdrSessionResult<HerdrSessionOperation[]>>("herdrSession", "listOperations");
}

export function controlOperation(request: { operationId: string; action: "cancel" | "revertBinding" }) {
  return invokeDesktop<HerdrSessionResult<HerdrSessionOperation>>("herdrSession", "controlOperation", request);
}
