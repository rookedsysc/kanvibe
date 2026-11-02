import { randomUUID } from "node:crypto";
import { buildTaskStatusNotification } from "@/desktop/shared/taskNotifications";
import { subscribeToBoardEvents } from "@/lib/boardNotifier";
import type { MobilePushSubscription } from "@/lib/mobilePairing";

export type MobileEvent = { type: "board-updated" } | {
  type: "task-status-changed";
  eventId: string;
  taskId: string;
  title: string;
  body: string;
  status: string;
  createdAt: string;
};

interface MobileNotificationOptions {
  relayUrl?: string;
  relayToken?: string;
  readSubscriptions: () => Promise<Array<MobilePushSubscription & { deviceId: string }>>;
  onUnregistered?: (deviceId: string, pushToken: string) => Promise<void>;
}

interface NotificationDelivery {
  isStopped: boolean;
  unsubscribe: () => void;
  requests: Set<AbortController>;
}

const mobileEventListeners = new Set<(event: MobileEvent) => void>();
const PUSH_TIMEOUT_MS = 5_000;
let currentDelivery: NotificationDelivery | null = null;

/** 소켓을 가진 네트워크 계층만 구독하고, hook 이벤트와 푸시는 같은 eventId를 공유한다 */
export function subscribeToMobileEvents(listener: (event: MobileEvent) => void): () => void {
  mobileEventListeners.add(listener);
  return () => { mobileEventListeners.delete(listener); };
}

function broadcastMobileEvent(event: MobileEvent): void {
  for (const listener of mobileEventListeners) {
    try {
      listener(event);
    } catch {
      console.error("[kanvibe] Mobile event delivery failed");
    }
  }
}

/** FCM의 notification·data·APNs alert에 문구가 반복되므로 UTF-8과 JSON 이스케이프 크기를 함께 제한한다 */
function truncateNotificationText(value: string, maxEncodedBytes: number): string {
  if (Buffer.byteLength(JSON.stringify(value)) - 2 <= maxEncodedBytes) return value;
  const characters: string[] = [];
  let encodedBytes = 0;
  for (const character of value) {
    const characterBytes = Buffer.byteLength(JSON.stringify(character)) - 2;
    if (encodedBytes + characterBytes > maxEncodedBytes - 3) break;
    characters.push(character);
    encodedBytes += characterBytes;
  }
  return `${characters.join("")}…`;
}

/** 서버 시작 시 설정을 명시적으로 받고 process.env나 터미널 환경을 바꾸지 않는다 */
export function startMobileNotifications(options: MobileNotificationOptions): void {
  stopMobileNotifications();
  const delivery: NotificationDelivery = { isStopped: false, unsubscribe: () => {}, requests: new Set() };
  currentDelivery = delivery;
  delivery.unsubscribe = subscribeToBoardEvents((event) => {
    if (event.type === "board-updated") {
      broadcastMobileEvent({ type: "board-updated" });
      return;
    }
    if (event.type !== "task-status-changed") return;

    const notification = buildTaskStatusNotification({ ...event, locale: "ko" });
    const mobileEvent: MobileEvent = {
      type: "task-status-changed", eventId: randomUUID(), taskId: event.taskId,
      title: truncateNotificationText(notification.title, 128), body: truncateNotificationText(notification.body, 700),
      status: event.newStatus, createdAt: new Date().toISOString(),
    };
    broadcastMobileEvent(mobileEvent);
    if (options.relayUrl && options.relayToken) {
      /** hook 응답과 보드 이벤트 발행은 외부 네트워크가 끝나기를 기다리지 않는다 */
      void deliverPushNotifications(mobileEvent, options, delivery).catch(() => {
        if (!delivery.isStopped) console.error("[kanvibe] Mobile push subscriptions could not be read");
      });
    }
  });
}

async function deliverPushNotifications(
  event: Extract<MobileEvent, { type: "task-status-changed" }>,
  options: MobileNotificationOptions,
  delivery: NotificationDelivery,
): Promise<void> {
  const subscriptions = await options.readSubscriptions();
  if (delivery.isStopped) return;
  await Promise.all(subscriptions.map(async (subscription) => {
    const controller = new AbortController();
    delivery.requests.add(controller);
    const timeout = setTimeout(() => controller.abort(), PUSH_TIMEOUT_MS);
    timeout.unref();
    try {
      const response = await fetch(options.relayUrl!, {
        method: "POST",
        headers: { "content-type": "application/json", authorization: `Bearer ${options.relayToken}` },
        body: JSON.stringify({
          token: subscription.pushToken,
          notification: { title: event.title, body: event.body },
          data: { ...event, connectionId: subscription.connectionId },
        }),
        signal: controller.signal,
      });
      if (response.status === 410) {
        if (!delivery.isStopped) await options.onUnregistered?.(subscription.deviceId, subscription.pushToken);
      } else if (!response.ok) {
        console.error("[kanvibe] Mobile push relay rejected notification:", response.status);
      }
      /** 응답 본문은 민감값을 포함할 수 있어 읽거나 로그에 실지 않는다 */
      await response.body?.cancel();
    } catch {
      if (!delivery.isStopped) console.error("[kanvibe] Mobile push delivery failed or timed out");
    } finally {
      clearTimeout(timeout);
      delivery.requests.delete(controller);
    }
  }));
}

/** 서버를 끄면 이벤트 리스너와 진행 중인 릴레이 요청을 함께 거둔다 */
export function stopMobileNotifications(): void {
  if (!currentDelivery) return;
  currentDelivery.isStopped = true;
  currentDelivery.unsubscribe();
  for (const request of currentDelivery.requests) request.abort();
  currentDelivery = null;
}
