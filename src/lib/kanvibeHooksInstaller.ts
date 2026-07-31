import { readTextFiles } from "@/lib/hostFileAccess";
import { getHookServerUrl } from "@/lib/hookEndpoint";
import { writeHookProviderFiles } from "@/lib/hookFileWriter";
import { registerKanvibeHookTarget } from "@/lib/hookTargetRegistration";
import {
  getKanvibeHookProviderModules,
  KANVIBE_HOOK_PROVIDER_MODULES,
  type KanvibeHookProvider,
  type KanvibeHookProviderModule,
  type KanvibeHookStatus,
} from "@/lib/kanvibeHookProviders";

const HOOK_INSTALL_MAX_ATTEMPTS = 3;
const HOOK_INSTALL_RETRY_DELAY_MS = 500;
const NON_BLOCKING_HOOK_STATUS_CHECKS = new Set([
  "hasReachableHookServer",
  "hasDuplicateKanvibePlugins",
  /** script의 KANVIBE_URL은 targets.json이 없을 때만 쓰는 fallback이므로 설치 실패로 보지 않는다 */
  "hasExpectedHookServerUrl",
]);
const OPEN_CODE_NON_BLOCKING_INSTALL_CHECKS = new Set([
  "hasRegisteredPlugin",
]);
const activeHookInstallJobs = new Map<string, Promise<void>>();
const activeHookFileInstallJobs = new Map<string, Promise<void>>();
const scheduledHookInstallJobs = new Map<string, ScheduledHookInstallJob>();

export type { KanvibeHookProvider };

interface HookInstallScheduleOptions {
  delayMs?: number;
  onSuccess?: () => void;
  onFailure?: (error: unknown) => void;
}

interface HookInstallCallbacks {
  onSuccess?: () => void;
  onFailure?: (error: unknown) => void;
}

interface ScheduledHookInstallJob {
  callbacks: HookInstallCallbacks[];
}

export async function installKanvibeHooks(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  return runDeduplicatedHookInstall(
    activeHookInstallJobs,
    buildHookInstallKey(targetPath, taskId, sshHost, "all"),
    () => runHookInstallWithRetry(
      "install",
      { targetPath, taskId, sshHost },
      async () => {
        await installKanvibeHookFilesOnce(targetPath, taskId, sshHost);
        await verifyHookInstallation(targetPath, taskId, sshHost);
      },
    ),
  );
}

export async function installKanvibeHookFiles(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  return runDeduplicatedHookInstall(
    activeHookFileInstallJobs,
    buildHookInstallKey(targetPath, taskId, sshHost, "all-files"),
    () => runHookInstallWithRetry(
      "file install",
      { targetPath, taskId, sshHost },
      () => installKanvibeHookFilesOnce(targetPath, taskId, sshHost),
    ),
  );
}

export async function installKanvibeHookProvider(
  targetPath: string,
  taskId: string,
  provider: KanvibeHookProvider,
  sshHost?: string | null,
): Promise<void> {
  const providerModule = KANVIBE_HOOK_PROVIDER_MODULES[provider];

  return runDeduplicatedHookInstall(
    activeHookInstallJobs,
    buildHookInstallKey(targetPath, taskId, sshHost, provider),
    () => runHookInstallWithRetry(
      "provider install",
      { targetPath, taskId, sshHost, provider: providerModule.label },
      () => installKanvibeHookProviderOnce(providerModule, targetPath, taskId, sshHost),
    ),
  );
}

export function scheduleKanvibeHooksInstall(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
  options: HookInstallScheduleOptions = {},
): void {
  const installKey = buildHookInstallKey(targetPath, taskId, sshHost, "all");
  const callbacks: HookInstallCallbacks = {
    onSuccess: options.onSuccess,
    onFailure: options.onFailure,
  };
  const scheduledJob = scheduledHookInstallJobs.get(installKey);
  if (scheduledJob) {
    scheduledJob.callbacks.push(callbacks);
    return;
  }

  const nextJob: ScheduledHookInstallJob = {
    callbacks: [callbacks],
  };
  scheduledHookInstallJobs.set(installKey, nextJob);

  setTimeout(() => {
    void installKanvibeHooks(targetPath, taskId, sshHost)
      .then(() => notifyHookInstallSuccess(nextJob.callbacks))
      .catch((error) => notifyHookInstallFailure(nextJob.callbacks, error))
      .finally(() => {
        if (scheduledHookInstallJobs.get(installKey) === nextJob) {
          scheduledHookInstallJobs.delete(installKey);
        }
      });
  }, options.delayMs ?? 0);
}

export function scheduleKanvibeHooksVerification(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
  options: HookInstallScheduleOptions = {},
): void {
  setTimeout(() => {
    void verifyHookInstallation(targetPath, taskId, sshHost)
      .then(() => {
        runHookInstallCallback(() => options.onSuccess?.());
      })
      .catch((error) => {
        runHookInstallCallback(() => options.onFailure?.(error));
      });
  }, options.delayMs ?? 0);
}

/** 같은 대상에 대한 설치 요청은 진행 중인 작업을 공유한다 */
async function runDeduplicatedHookInstall(
  activeJobs: Map<string, Promise<void>>,
  installKey: string,
  startJob: () => Promise<void>,
): Promise<void> {
  const activeJob = activeJobs.get(installKey);
  if (activeJob) {
    return activeJob;
  }

  const installJob = startJob().finally(() => {
    if (activeJobs.get(installKey) === installJob) {
      activeJobs.delete(installKey);
    }
  });

  activeJobs.set(installKey, installJob);
  return installJob;
}

function buildHookInstallKey(
  targetPath: string,
  taskId: string,
  sshHost: string | null | undefined,
  provider: KanvibeHookProvider | "all" | "all-files",
): string {
  return [provider, sshHost ?? "", targetPath, taskId].join("\0");
}

function notifyHookInstallSuccess(callbacks: HookInstallCallbacks[]): void {
  for (const callback of callbacks) {
    runHookInstallCallback(() => callback.onSuccess?.());
  }
}

function notifyHookInstallFailure(callbacks: HookInstallCallbacks[], error: unknown): void {
  for (const callback of callbacks) {
    runHookInstallCallback(() => callback.onFailure?.(error));
  }
}

function runHookInstallCallback(callback: () => void): void {
  try {
    callback();
  } catch (error) {
    console.warn("[hooks] install callback failed", {
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

interface HookInstallContext {
  targetPath: string;
  taskId: string;
  sshHost?: string | null;
  provider?: string;
}

async function runHookInstallWithRetry(
  operationLabel: string,
  context: HookInstallContext,
  install: () => Promise<void>,
): Promise<void> {
  let lastError: unknown;

  for (let attempt = 1; attempt <= HOOK_INSTALL_MAX_ATTEMPTS; attempt += 1) {
    try {
      await install();
      return;
    } catch (error) {
      lastError = error;

      if (attempt === HOOK_INSTALL_MAX_ATTEMPTS) {
        throw error;
      }

      const retryDelayMs = HOOK_INSTALL_RETRY_DELAY_MS * attempt;
      console.warn(`[hooks] ${operationLabel} failed; retrying`, {
        ...context,
        sshHost: context.sshHost ?? null,
        attempt,
        maxAttempts: HOOK_INSTALL_MAX_ATTEMPTS,
        nextAttemptInMs: retryDelayMs,
        error: error instanceof Error ? error.message : String(error),
      });
      await waitForHookInstallRetry(retryDelayMs);
    }
  }

  throw lastError instanceof Error ? lastError : new Error("hooks 설정 실패");
}

async function waitForHookInstallRetry(delayMs: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, delayMs));
}

/**
 * 모든 provider의 hook 파일을 한 번에 만들어 기록한다.
 * 기존 설정 파일 읽기와 파일 쓰기를 각각 한 번으로 묶어 원격 설치의 왕복 횟수를 줄인다.
 */
async function installKanvibeHookFilesOnce(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);
  await registerKanvibeHookTarget(targetPath, taskId, hookServerUrl, sshHost);

  const providerModules = getKanvibeHookProviderModules();
  const existingFiles = await readTextFiles(
    providerModules.flatMap((providerModule) => providerModule.getInstallInputPaths(targetPath, sshHost)),
    sshHost,
  );

  const buildResults = providerModules.map((providerModule) => {
    try {
      return providerModule.buildFiles(targetPath, taskId, hookServerUrl, existingFiles, sshHost);
    } catch (error) {
      console.error(`[hooks] ${providerModule.label} install failed`, {
        provider: providerModule.label,
        targetPath,
        taskId,
        sshHost: sshHost ?? null,
        error: error instanceof Error ? error.message : String(error),
      });
      throw error;
    }
  });

  await writeHookProviderFiles(buildResults.flat(), sshHost);
}

async function installKanvibeHookProviderOnce(
  providerModule: KanvibeHookProviderModule,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);

  await providerModule.install(targetPath, taskId, hookServerUrl, sshHost);
  await verifyHookProviderInstallation(providerModule, targetPath, taskId, sshHost);
}

interface HookVerificationFailure {
  provider: string;
  failedChecks?: string[];
  error?: unknown;
}

async function verifyHookInstallation(targetPath: string, taskId: string, sshHost?: string | null) {
  const failures = await logHookVerificationStatuses(targetPath, taskId, sshHost);
  if (failures.length > 0) {
    throw new Error(`hooks 검증 실패: ${formatHookVerificationFailures(failures)}`);
  }
}

async function verifyHookProviderInstallation(
  providerModule: KanvibeHookProviderModule,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
) {
  const failure = await logHookProviderVerificationStatus(providerModule, targetPath, taskId, sshHost);
  if (failure) {
    throw new Error(`hooks 검증 실패: ${formatHookVerificationFailures([failure])}`);
  }
}

async function logHookVerificationStatuses(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<HookVerificationFailure[]> {
  const providerModules = getKanvibeHookProviderModules();
  const results = await Promise.allSettled(
    providerModules.map((providerModule) => providerModule.getStatus(targetPath, taskId, sshHost)),
  );
  const failures: HookVerificationFailure[] = [];

  for (const [index, result] of results.entries()) {
    const provider = providerModules[index].label;
    if (result.status === "fulfilled") {
      const failure = evaluateHookVerificationStatus(provider, result.value, targetPath, taskId, sshHost);
      if (failure) {
        failures.push(failure);
      }
      continue;
    }

    console.warn(`[hooks] ${provider} verification unavailable`, {
      provider,
      targetPath,
      taskId,
      sshHost: sshHost ?? null,
      error: result.reason instanceof Error ? result.reason.message : String(result.reason),
    });
    failures.push({ provider, error: result.reason });
  }

  return failures;
}

async function logHookProviderVerificationStatus(
  providerModule: KanvibeHookProviderModule,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<HookVerificationFailure | null> {
  try {
    const status = await providerModule.getStatus(targetPath, taskId, sshHost);
    return evaluateHookVerificationStatus(providerModule.label, status, targetPath, taskId, sshHost);
  } catch (error) {
    console.warn(`[hooks] ${providerModule.label} verification unavailable`, {
      provider: providerModule.label,
      targetPath,
      taskId,
      sshHost: sshHost ?? null,
      error: error instanceof Error ? error.message : String(error),
    });
    return { provider: providerModule.label, error };
  }
}

function evaluateHookVerificationStatus(
  provider: string,
  status: KanvibeHookStatus,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): HookVerificationFailure | null {
  const failedChecks = logHookVerificationStatus(provider, status, targetPath, taskId, sshHost);
  const fatalFailedChecks = getFatalHookVerificationFailedChecks(provider, status, failedChecks);

  return shouldFailHookVerification(status, failedChecks, fatalFailedChecks)
    ? { provider, failedChecks: fatalFailedChecks }
    : null;
}

function logHookVerificationStatus(
  provider: string,
  status: KanvibeHookStatus,
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): string[] {
  const failedChecks = getHookVerificationFailedChecks(status);
  const payload = {
    provider,
    targetPath,
    taskId,
    sshHost: sshHost ?? null,
    installed: status.installed,
    failedChecks,
    boundTaskId: status.boundTaskId ?? null,
    registeredHookTargetUrl: status.registeredHookTargetUrl ?? null,
    configuredHookServerUrl: status.configuredHookServerUrl ?? null,
    expectedHookServerUrl: status.expectedHookServerUrl ?? null,
    registeredPluginUrls: "registeredPluginUrls" in status && Array.isArray(status.registeredPluginUrls)
      ? status.registeredPluginUrls
      : undefined,
  };

  if (status.installed) {
    console.log(`[hooks] ${provider} verification`, payload);
    return failedChecks;
  }

  console.warn(`[hooks] ${provider} verification`, payload);
  return failedChecks;
}

function getHookVerificationFailedChecks(status: KanvibeHookStatus): string[] {
  return Object.entries(status)
    .filter(([key, value]) => key.startsWith("has") && value === false)
    .filter(([key]) => !NON_BLOCKING_HOOK_STATUS_CHECKS.has(key))
    .map(([key]) => key);
}

function getFatalHookVerificationFailedChecks(
  provider: string,
  status: KanvibeHookStatus,
  failedChecks: string[],
): string[] {
  if (status.installed || provider !== "OpenCode") {
    return failedChecks;
  }

  return failedChecks.filter((check) => !OPEN_CODE_NON_BLOCKING_INSTALL_CHECKS.has(check));
}

function shouldFailHookVerification(
  status: KanvibeHookStatus,
  failedChecks: string[],
  fatalFailedChecks: string[],
): boolean {
  if (status.installed) {
    return false;
  }

  return fatalFailedChecks.length > 0 || failedChecks.length === 0;
}

function formatHookVerificationFailures(failures: HookVerificationFailure[]): string {
  return failures.map(({ provider, failedChecks, error }) => {
    if (error) {
      return `${provider}(${error instanceof Error ? error.message : String(error)})`;
    }

    if (failedChecks && failedChecks.length > 0) {
      return `${provider}(${failedChecks.join(", ")})`;
    }

    return provider;
  }).join(", ");
}
