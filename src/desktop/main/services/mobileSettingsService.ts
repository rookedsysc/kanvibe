import { SessionType } from "@/entities/KanbanTask";
import { BOARD_SORT_FIELDS, BOARD_SORT_DIRECTIONS, type BoardSortPreference } from "@/desktop/shared/boardSort";
import { getDefaultSessionType, setDefaultSessionType, getDoneAlertDismissed, dismissDoneAlert, getBoardSortPreference, setBoardSortPreference, getBackgroundSyncEnabled, setBackgroundSyncEnabled, getBackgroundSyncIntervalMs, setBackgroundSyncIntervalMs } from "./appSettingsService";
import { getSessionTypeChangeTargets, changeAllTaskSessionTypes, deleteTasks } from "./kanbanService";
import { getSessionDependencyStatus } from "@/lib/remoteSessionDependency";
import { runBackgroundTaskSyncNow } from "./backgroundTaskSyncService";
export { getBackgroundTaskSyncStatus as getMobileSyncStatus, startBackgroundTaskSyncNow as startMobileSync } from "./backgroundTaskSyncService";
import { getNotificationById, markTaskNotificationsRead } from "@/desktop/main/notificationStore";
import { MobileFeatureError, readMobileBoolean, readMobileEnum, readMobileObject, readMobileText, requireMobileTask } from "./mobileFeatureValidation";

export async function getMobileSettings() {
  const [defaultSessionType, sortPreference, doneAlertDismissed, isEnabled, intervalMs] = await Promise.all([
    getDefaultSessionType(), getBoardSortPreference(), getDoneAlertDismissed(), getBackgroundSyncEnabled(), getBackgroundSyncIntervalMs(),
  ]);
  return { defaultSessionType, sortPreference, doneAlertDismissed, backgroundSync: { isEnabled, intervalMs } };
}

function readSortPreference(input: unknown): BoardSortPreference {
  const body = readMobileObject(input);
  if (!Array.isArray(body.keys) || body.keys.length > BOARD_SORT_FIELDS.length) throw new MobileFeatureError(400, "정렬 값이 올바르지 않습니다");
  const keys = body.keys.map((value) => {
    const key = readMobileObject(value);
    return { field: readMobileEnum(BOARD_SORT_FIELDS, key.field, "정렬 기준"), direction: readMobileEnum(BOARD_SORT_DIRECTIONS, key.direction, "정렬 방향") };
  });
  if (new Set(keys.map((key) => key.field)).size !== keys.length) throw new MobileFeatureError(400, "정렬 기준이 중복되었습니다");
  return { keys };
}

export async function updateMobileSettings(input: unknown) {
  const body = readMobileObject(input);
  if (!Object.keys(body).length || Object.keys(body).some((key) => !["defaultSessionType", "sortPreference", "doneAlertDismissed", "backgroundSync"].includes(key))) throw new MobileFeatureError(400, "지원하지 않는 설정입니다");
  const sessionType = "defaultSessionType" in body ? readMobileEnum(Object.values(SessionType), body.defaultSessionType, "기본 세션") : undefined;
  const sortPreference = "sortPreference" in body ? readSortPreference(body.sortPreference) : undefined;
  const doneAlertDismissed = "doneAlertDismissed" in body ? readMobileBoolean(body.doneAlertDismissed, "Done 안내") : undefined;
  if (doneAlertDismissed === false) throw new MobileFeatureError(400, "Done 안내 숨김은 켜기만 지원합니다");
  let backgroundSync: { isEnabled: boolean; intervalMs: number } | undefined;
  if ("backgroundSync" in body) {
    const settings = readMobileObject(body.backgroundSync);
    const isEnabled = readMobileBoolean(settings.isEnabled, "동기화 사용");
    if (!Number.isSafeInteger(settings.intervalMs) || (settings.intervalMs as number) < 60000 || (settings.intervalMs as number) > 86400000) throw new MobileFeatureError(400, "동기화 주기는 1분에서 1440분 사이여야 합니다");
    backgroundSync = { isEnabled, intervalMs: settings.intervalMs as number };
  }
  if (sessionType) await setDefaultSessionType(sessionType);
  if (sortPreference) await setBoardSortPreference(sortPreference);
  if (doneAlertDismissed) await dismissDoneAlert();
  if (backgroundSync) {
    await setBackgroundSyncIntervalMs(backgroundSync.intervalMs);
    await setBackgroundSyncEnabled(backgroundSync.isEnabled);
  }
  return getMobileSettings();
}

export async function getMobileSessionChangeTargets(input: unknown) {
  return getSessionTypeChangeTargets(readMobileEnum(Object.values(SessionType), input, "세션 종류"));
}

export async function changeAllMobileTaskSessions(input: unknown) {
  const body = readMobileObject(input);
  const sessionType = readMobileEnum(Object.values(SessionType), body.sessionType, "세션 종류");
  const shouldClose = readMobileBoolean(body.shouldCloseExistingSessions, "기존 세션 종료");
  const targets = await getSessionTypeChangeTargets(sessionType);
  for (const host of targets.sshHosts) {
    if (!(await getSessionDependencyStatus(sessionType, host)).available) throw new MobileFeatureError(409, "데스크탑에서 모든 대상의 세션 도구를 설치해 주세요");
  }
  return changeAllTaskSessionTypes(sessionType, shouldClose);
}

export async function runMobileSync() {
  return runBackgroundTaskSyncNow();
}

export async function getMobileSyncReview(notificationId: string) {
  const notification = await getNotificationById(readMobileText(notificationId, "알림", 128));
  if (!notification) throw new MobileFeatureError(404, "없는 알림입니다");
  if (notification.action?.type !== "background-sync-review") throw new MobileFeatureError(409, "동기화 검토 알림이 아닙니다");
  return notification.action.payload;
}

export async function deleteMobileSyncReviewTasks(notificationId: string, input: unknown) {
  const body = readMobileObject(input);
  if (!Array.isArray(body.taskIds) || body.taskIds.length > 100) throw new MobileFeatureError(400, "선택한 태스크가 올바르지 않습니다");
  const taskIds = [...new Set(body.taskIds.map((id) => readMobileText(id, "태스크", 128)))];
  const review = await getMobileSyncReview(notificationId);
  for (const taskId of taskIds) {
    const mergedPr = review.mergedPullRequests.find((entry) => entry.taskId === taskId);
    if (!mergedPr) throw new MobileFeatureError(400, "검토 목록에 있는 태스크만 삭제할 수 있습니다");
    const task = await requireMobileTask(taskId);
    if (task.branchName !== mergedPr.branchName || (task.prUrl && task.prUrl !== mergedPr.prUrl)) throw new MobileFeatureError(409, "태스크가 변경되었습니다. 최신 검토 내용을 확인해 주세요");
  }
  return deleteTasks(taskIds);
}

export async function markMobileTaskNotificationsRead(taskId: string) {
  await requireMobileTask(taskId);
  return markTaskNotificationsRead(taskId);
}
