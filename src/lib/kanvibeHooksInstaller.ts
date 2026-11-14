import { createHash } from "node:crypto";
import { getOrCreateHookClientId } from "@/desktop/main/services/appSettingsService";
import { HOOK_TRANSPORT_VERSION_MARKER } from "@/lib/hookSocketPath";
import { invalidateOpenCodePluginRegistration } from "@/lib/openCodePluginRegistry";
import { readTextFiles } from "@/lib/hostFileAccess";
import { runWithBackgroundSSHCommands } from "@/lib/gitOperations";
import { getHookServerPort, getHookServerUrl } from "@/lib/hookEndpoint";
import { writeHookProviderFiles } from "@/lib/hookFileWriter";
import { registerKanvibeHookTarget } from "@/lib/hookTargetRegistration";
import { assertKanvibeStatePathsAreNotSymlinks, getKanvibeTargetsPath, parseKanvibeTargets } from "@/lib/kanvibeProjectState";
import { buildTaskCliFile, TASK_CLI_FILE_NAME } from "@/lib/taskCliScript";
import {
  getKanvibeHookProviderModules,
  KANVIBE_HOOK_PROVIDER_MODULES,
  type KanvibeHookProvider,
  type KanvibeHookProviderModule,
  type KanvibeHookStatus,
  type KanvibeHookStatuses,
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
const activeHookEnsureJobs = new Map<string, Promise<boolean>>();
const verifiedRemoteHookFiles = new Map<string, string>();
const activeHookInstallJobs = new Map<string, Promise<void>>();
const activeHookFileInstallJobs = new Map<string, Promise<void>>();
const activeHookStatusReads = new Map<string, Promise<KanvibeHookStatuses>>();
/**
 * 원격 hook 상태를 다시 읽지 않고 돌려주는 시간. 상태 패널이 열린 창은 보드 갱신마다 상태를 다시 묻는데,
 * 원격에서는 조회 한 번이 새 SSH 접속이라 에이전트가 일하는 동안 접속이 끊임없이 이어진다.
 * 이 앱이 설치하거나 복구하면 곧바로 버리므로, 이 시간만큼 늦게 보이는 것은 원격에서 직접 고친 변경뿐이다.
 */
const REMOTE_HOOK_STATUS_REUSE_MS = 30_000;
const recentRemoteHookStatuses = new Map<string, { readAt: number; statuses: KanvibeHookStatuses }>();
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

/** 현재 설치를 확인하고 구형 파일 또는 현재 client 등록을 복구했다면 true를 반환한다. 검사 실패는 재시도할 수 있도록 던진다. */
export async function ensureKanvibeHooksCurrent(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<boolean> {
  return runDeduplicatedHookInstall(
    activeHookEnsureJobs,
    buildHookInstallKey(targetPath, taskId, sshHost, "all"),
    async () => {
      if ((await inspectKanvibeHooksState(targetPath, taskId, sshHost)).isCurrent) return false;
      await installKanvibeHooks(targetPath, taskId, sshHost);
      const verification = await inspectKanvibeHooksState(targetPath, taskId, sshHost);
      if (!verification.isCurrent) {
        throw new Error(`hooks 검증 실패: ${verification.failures.join(", ")}`);
      }
      return true;
    },
  );
}

/**
 * 모든 provider의 hook 상태를 파일 읽기 한 번으로 조회한다.
 * 여러 창이나 화면이 같은 대상을 동시에 물으면 진행 중인 조회를 함께 쓴다. 원격 조회는 호스트 키를 검증하는
 * 새 SSH 접속이라, 겹친 요청마다 따로 접속하면 호스트별 인증 순서에 줄을 서서 뒤 요청일수록 오래 걸린다.
 */
export function getKanvibeHooksStatuses(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<KanvibeHookStatuses> {
  const statusKey = buildHookInstallKey(targetPath, taskId, sshHost, "status");
  const recentStatuses = recentRemoteHookStatuses.get(statusKey);
  if (recentStatuses && Date.now() - recentStatuses.readAt < REMOTE_HOOK_STATUS_REUSE_MS) {
    return Promise.resolve(recentStatuses.statuses);
  }

  const activeRead = activeHookStatusReads.get(statusKey);
  if (activeRead) {
    return activeRead;
  }

  const statusRead = readKanvibeHooksStatuses(targetPath, taskId, sshHost).then((statuses) => {
    // 읽는 사이 설치가 끝나 이 조회가 버려졌으면 설치 전 상태일 수 있으므로 기억하지 않는다.
    if (sshHost && activeHookStatusReads.get(statusKey) === statusRead) {
      recentRemoteHookStatuses.set(statusKey, { readAt: Date.now(), statuses });
    }
    return statuses;
  }).finally(() => {
    if (activeHookStatusReads.get(statusKey) === statusRead) {
      activeHookStatusReads.delete(statusKey);
    }
  });
  activeHookStatusReads.set(statusKey, statusRead);
  return statusRead;
}

/** 설치가 끝난 뒤의 조회가 설치 전에 읽었거나 읽기 시작한 상태를 받지 않게 한다 */
function forgetHookStatusReads(targetPath: string, taskId: string, sshHost?: string | null): void {
  const statusKey = buildHookInstallKey(targetPath, taskId, sshHost, "status");
  activeHookStatusReads.delete(statusKey);
  recentRemoteHookStatuses.delete(statusKey);
}

/** provider들이 상태 판정에 쓰는 파일을 한 번에 읽는다 */
function readHookStatusFiles(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): ReturnType<typeof readTextFiles> {
  const providerPaths = getKanvibeHookProviderModules().flatMap((providerModule) => (
    providerModule.buildFiles(targetPath, taskId, "", new Map(), sshHost).map((file) => file.filePath)
  ));
  const paths = [...new Set([...providerPaths, getKanvibeTargetsPath(targetPath, sshHost)])];
  return readTextFiles(paths, sshHost, { strictHostKeyChecking: true });
}

async function readKanvibeHooksStatuses(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<KanvibeHookStatuses> {
  const files = await readHookStatusFiles(targetPath, taskId, sshHost);
  const [claude, gemini, codex, openCode] = await Promise.all([
    KANVIBE_HOOK_PROVIDER_MODULES.claude.getStatus(targetPath, taskId, sshHost, files),
    KANVIBE_HOOK_PROVIDER_MODULES.gemini.getStatus(targetPath, taskId, sshHost, files),
    KANVIBE_HOOK_PROVIDER_MODULES.codex.getStatus(targetPath, taskId, sshHost, files),
    KANVIBE_HOOK_PROVIDER_MODULES.openCode.getStatus(targetPath, taskId, sshHost, files),
  ]);
  return { claude, gemini, codex, openCode };
}

async function inspectKanvibeHooksState(
  targetPath: string,
  taskId: string,
  sshHost?: string | null,
): Promise<{ isCurrent: boolean; failures: string[] }> {
  const cliPath = buildTaskCliFile(targetPath, sshHost).filePath;
  const targetsPath = getKanvibeTargetsPath(targetPath, sshHost);
  const providerModules = getKanvibeHookProviderModules();
  const providerPaths = sshHost ? providerModules.flatMap((providerModule) => (
    providerModule.buildFiles(targetPath, taskId, "", new Map(), sshHost).map((file) => file.filePath)
  )) : [];
  const verificationPaths = [...new Set([...providerPaths, cliPath, targetsPath])];
  const [files, clientId] = await Promise.all([
    readTextFiles(verificationPaths, sshHost, { strictHostKeyChecking: true }),
    getOrCreateHookClientId(),
  ]);
  const verificationKey = buildHookInstallKey(targetPath, taskId, sshHost, "all");
  const hookServerPort = getHookServerPort();
  const fingerprint = getHookFilesFingerprint(files, clientId, hookServerPort);
  if (sshHost && verifiedRemoteHookFiles.get(verificationKey) === fingerprint) return { isCurrent: true, failures: [] };
  verifiedRemoteHookFiles.delete(verificationKey);
  const statuses = await Promise.all(providerModules.map(async (providerModule) => ({
    providerModule,
    status: await providerModule.getStatus(targetPath, taskId, sshHost, ...(sshHost ? [files] : [])),
  })));
  const failures = statuses.flatMap(({ providerModule, status }) => {
    const failedChecks = getHookVerificationFailedChecks(status);
    // 등록의 IP는 재접속 때 바뀔 수 있으므로 현재 client ID로 확인한다.
    const fileChecks = getFatalHookVerificationFailedChecks(providerModule.label, status, failedChecks)
      .filter((check) => check !== "hasRegisteredHookTarget" || !sshHost);
    if (!status.installed && failedChecks.length === 0) {
      const hasUnavailableConnection = status.hasReachableHookServer === false || status.hasExpectedHookServerUrl === false;
      if (!hasUnavailableConnection) throw new Error(`${providerModule.label} hooks 검사 결과를 확인할 수 없습니다.`);
    }
    return fileChecks.length > 0 ? [`${providerModule.label}(${fileChecks.join(", ")})`] : [];
  });
  const cliFile = files.get(cliPath);
  const hasCurrentCli = cliFile?.exists === true && cliFile.content.includes(HOOK_TRANSPORT_VERSION_MARKER);
  const ownTarget = parseKanvibeTargets(files.get(targetsPath)?.content ?? "").targets
    .find((target) => target.clientId === clientId && target.taskId === taskId);
  const hasCurrentTarget = Boolean(ownTarget?.token) && ownTarget?.transport === (sshHost ? "ssh" : "local");
  if (!hasCurrentCli) failures.push("task.sh(최신 전송 파일이 없습니다)");
  if (!hasCurrentTarget) failures.push("targets.json(현재 client 등록이 없습니다)");
  const isCurrent = failures.length === 0;
  // provider가 같은 파일 묶음을 검증했을 때만 재사용해, 검사 도중 바뀐 파일을 정상으로 기억하지 않는다.
  if (sshHost && isCurrent) verifiedRemoteHookFiles.set(verificationKey, fingerprint);
  return { isCurrent, failures };
}

function getHookFilesFingerprint(
  files: Awaited<ReturnType<typeof readTextFiles>>,
  clientId: string,
  hookServerPort: number,
): string {
  return createHash("sha256").update(JSON.stringify([clientId, hookServerPort, [...files]])).digest("hex");
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
  ).finally(() => forgetHookStatusReads(targetPath, taskId, sshHost));
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
  ).finally(() => forgetHookStatusReads(targetPath, taskId, sshHost));
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
  ).finally(() => forgetHookStatusReads(targetPath, taskId, sshHost));
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
    // 예약된 설치는 기다리는 화면이 없으므로, 터미널 준비 같은 화면 요청이 원격 명령 슬롯을 먼저 쓰게 한다.
    void runWithBackgroundSSHCommands(() => installKanvibeHooks(targetPath, taskId, sshHost))
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
    void runWithBackgroundSSHCommands(() => verifyHookInstallation(targetPath, taskId, sshHost))
      .then(() => {
        runHookInstallCallback(() => options.onSuccess?.());
      })
      .catch((error) => {
        runHookInstallCallback(() => options.onFailure?.(error));
      });
  }, options.delayMs ?? 0);
}

/** 같은 대상에 대한 설치 요청은 진행 중인 작업을 공유한다 */
async function runDeduplicatedHookInstall<T>(
  activeJobs: Map<string, Promise<T>>,
  installKey: string,
  startJob: () => Promise<T>,
): Promise<T> {
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
  provider: KanvibeHookProvider | "all" | "all-files" | "status",
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
 * 모든 provider의 hook 파일과 task CLI를 만들어 기록한다.
 * 기존 설정 파일 읽기는 한 번으로 묶고, 쓰기는 provider 단위로 격리해
 * 한 provider가 실패해도 나머지 provider의 hook은 설치되게 한다.
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
    { strictHostKeyChecking: true },
  );

  const providerWrites = providerModules.map(async (providerModule) => {
    try {
      const files = providerModule.buildFiles(targetPath, taskId, hookServerUrl, existingFiles, sshHost);
      await writeHookProviderFiles(files, sshHost);
    } catch (error) {
      console.error(`[hooks] ${providerModule.label} install failed`, {
        provider: providerModule.label,
        targetPath,
        taskId,
        sshHost: sshHost ?? null,
        error: error instanceof Error ? error.message : String(error),
      });
      throw error;
    } finally {
      if (providerModule.provider === "openCode" && !sshHost) {
        invalidateOpenCodePluginRegistration(targetPath);
      }
    }
  });
  /** task CLI는 provider와 무관하게 worktree마다 하나만 두고, 원격 왕복이 늘지 않도록 provider 쓰기와 함께 보낸다 */
  const taskCliWrite = assertKanvibeStatePathsAreNotSymlinks(targetPath, [TASK_CLI_FILE_NAME], sshHost)
    .then(() => writeHookProviderFiles([buildTaskCliFile(targetPath, sshHost)], sshHost));
  const results = await Promise.allSettled([...providerWrites, taskCliWrite]);

  assertHookProviderInstallResults(results, [...providerModules.map(({ label }) => label), "task CLI"]);
}

/** provider 하나라도 실패하면 실패한 provider를 모두 모아 알린다 */
function assertHookProviderInstallResults(
  results: PromiseSettledResult<void>[],
  providerLabels: string[],
): void {
  const failedProviders = results.flatMap((result, index) => (
    result.status === "rejected"
      ? [`${providerLabels[index]}(${result.reason instanceof Error ? result.reason.message : String(result.reason)})`]
      : []
  ));

  if (failedProviders.length > 0) {
    throw new Error(`hooks 설치 실패: ${failedProviders.join(", ")}`);
  }
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
  /**
   * 원격에서는 provider마다 따로 읽으면 검증 접속이 provider 수만큼 차례로 생기므로 한 번 읽어 나눠 쓴다.
   * 묶어 읽기가 실패하면 어느 provider가 원인인지 가릴 수 없어, 그때만 provider별로 다시 읽는다.
   */
  const sharedFiles = sshHost
    ? await readHookStatusFiles(targetPath, taskId, sshHost).catch(() => null)
    : null;
  const results = await Promise.allSettled(
    providerModules.map((providerModule) => (
      providerModule.getStatus(targetPath, taskId, sshHost, ...(sharedFiles ? [sharedFiles] : []))
    )),
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
    pluginRegistrationError: getPluginRegistrationError(status),
  };

  if (status.installed) {
    console.log(`[hooks] ${provider} verification`, payload);
    return failedChecks;
  }

  console.warn(`[hooks] ${provider} verification`, payload);
  return failedChecks;
}

function getHookVerificationFailedChecks(status: KanvibeHookStatus): string[] {
  const failedChecks = Object.entries(status)
    .filter(([key, value]) => key.startsWith("has") && value === false)
    .filter(([key]) => !NON_BLOCKING_HOOK_STATUS_CHECKS.has(key))
    .map(([key]) => key);

  /**
   * OpenCode CLI가 없거나 첫 실행의 의존성 설치로 조회가 제한 시간을 넘기면 등록 여부를 알 수 없어
   * hasRegisteredPlugin이 비어 있다. 이 미확인 상태를 등록 누락과 같은 비차단 항목으로 세어야
   * 실패 항목 0개인 미설치로 읽혀 task 생성이 막히지 않는다.
   */
  if (getPluginRegistrationError(status) && !failedChecks.includes("hasRegisteredPlugin")) {
    failedChecks.push("hasRegisteredPlugin");
  }

  return failedChecks;
}

function getPluginRegistrationError(status: KanvibeHookStatus): string | undefined {
  return "pluginRegistrationError" in status ? status.pluginRegistrationError : undefined;
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
