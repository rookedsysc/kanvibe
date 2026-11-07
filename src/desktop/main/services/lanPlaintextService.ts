import {
  getLanPlaintextAllowed,
  getLanPlaintextLastRequestAt,
  getOrStartLanPlaintextObservation,
  setLanPlaintextLastRequestAt,
} from "@/desktop/main/services/appSettingsService";

/**
 * 평문 hook 포트를 LAN에 열어 둘지 정한다.
 *
 * 터널이 안 되는 호스트와 재페어링 전의 모바일은 아직 평문으로 들어오므로, 이 설치에서 LAN 평문 요청이 일정 기간
 * 없었을 때만 루프백으로 닫는다. 사용량은 이 기기 안에서만 세며 어디로도 보내지 않는다.
 * 터널을 쓸 수 없는 Windows는 평문 외의 경로가 없어 닫지 않는다.
 */
export const LAN_PLAINTEXT_IDLE_WINDOW_MS = 14 * 24 * 60 * 60_000;
/** 요청마다 DB에 쓰지 않도록 기록 간격을 둔다 */
const LAN_PLAINTEXT_RECORD_INTERVAL_MS = 60 * 60_000;
const LAN_BIND_HOST = "0.0.0.0";
const LOOPBACK_BIND_HOST = "127.0.0.1";

let lastRecordedRequestAt = 0;

export async function resolveHookServerBindHost(now = Date.now()): Promise<string> {
  try {
    if (process.platform === "win32" || await getLanPlaintextAllowed()) {
      return LAN_BIND_HOST;
    }

    const observedSince = await getOrStartLanPlaintextObservation(now);
    const lastRequestAt = await getLanPlaintextLastRequestAt();
    const isIdle = now - Math.max(observedSince, lastRequestAt) >= LAN_PLAINTEXT_IDLE_WINDOW_MS;
    return isIdle ? LOOPBACK_BIND_HOST : LAN_BIND_HOST;
  } catch (error) {
    /** 사용량을 읽지 못했으면 닫을 근거가 없으므로 기존 동작을 유지한다. 닫으면 원격 hook이 조용히 끊긴다 */
    console.warn("[kanvibe] LAN plaintext check failed:", error instanceof Error ? error.message : error);
    return LAN_BIND_HOST;
  }
}

function isLoopbackAddress(remoteAddress: string | undefined): boolean {
  return remoteAddress === "127.0.0.1" || remoteAddress === "::1" || remoteAddress === "::ffff:127.0.0.1";
}

export function recordLanPlaintextRequest(remoteAddress: string | undefined, now = Date.now()): void {
  if (isLoopbackAddress(remoteAddress) || now - lastRecordedRequestAt < LAN_PLAINTEXT_RECORD_INTERVAL_MS) {
    return;
  }
  lastRecordedRequestAt = now;
  setLanPlaintextLastRequestAt(now).catch((error) => {
    console.warn("[kanvibe] LAN plaintext usage record failed:", error instanceof Error ? error.message : error);
  });
}

