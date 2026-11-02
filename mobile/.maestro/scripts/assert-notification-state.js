// 데스크톱 renderer IPC가 읽은 실제 저장 상태와 변경 신호를 확인한다. ID는 fixture만 제공한다.
const previousEvents = Number(output.desktopNotificationEventCount || 0);
const expectedUnread = Number(EXPECT_UNREAD);
const expectedTotal = Number(EXPECT_TOTAL);
const requiresEvent = String(EXPECT_EVENT) === "true";
const query = [];
if (expectedUnread >= 0) query.push(`unread=${expectedUnread}`);
if (requiresEvent) query.push(`eventsAfter=${previousEvents}`);
const suffix = query.length ? `?${query.join("&")}` : "";
const action = String(DESKTOP_ACTION);
const allowedActions = ["", "read-one", "read-all", "create", "update-task"];
if (allowedActions.indexOf(action) < 0) throw new Error(`알 수 없는 fixture 동작: ${action}`);
const response = action
  ? http.post(`${PROBE_API_FROM_HOST}/desktop/${action}${suffix}`, { body: "" })
  : http.get(`${PROBE_API_FROM_HOST}/state${suffix}`);
if (response.status !== 200) throw new Error(`알림 probe 실패: HTTP ${response.status} ${response.body}`);
const state = json(response.body);
if (expectedUnread >= 0 && state.unreadCount !== expectedUnread) {
  throw new Error(`미읽음 수 불일치: expected=${expectedUnread}, actual=${state.unreadCount}`);
}
if (expectedTotal >= 0 && state.notifications.length !== expectedTotal) {
  throw new Error(`알림 수 불일치: expected=${expectedTotal}, actual=${state.notifications.length}`);
}
if (requiresEvent && state.desktopEventCount <= previousEvents) {
  throw new Error(`실제 desktop renderer 변경 신호 누락: ${response.body}`);
}
const expectedRead = String(EXPECT_READ);
if (expectedRead) {
  const seeded = state.fixture.notifications[expectedRead];
  const notification = seeded && state.notifications.find((entry) => entry.id === seeded.id);
  if (!notification || !notification.isRead) throw new Error(`데스크톱 읽음 상태 누락: ${expectedRead}`);
}
output.alphaProjectId = state.fixture.projects.alpha.id;
output.betaProjectId = state.fixture.projects.beta.id;
output.mobileReadNotificationId = state.fixture.notifications.mobileRead.id;
output.desktopReadNotificationId = state.fixture.notifications.desktopRead.id;
output.openTaskNotificationId = state.fixture.notifications.openTask.id;
output.alreadyReadNotificationId = state.fixture.notifications.alreadyRead.id;
output.latestTaskTitle = state.fixture.latestTaskTitle;
const live = state.notifications.find((entry) => entry.title === state.fixture.liveNotificationTitle);
output.liveNotificationId = live ? live.id : "";
output.desktopNotificationEventCount = state.desktopEventCount;
