"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  listPairedMobileDevices,
  startMobilePairing,
  stopMobilePairing,
  unpairMobileDevice,
  type PairedMobileDevice,
} from "@/desktop/renderer/actions/mobileBridge";

/**
 * 모바일 앱을 이 데스크탑에 연결하는 설정.
 *
 * 코드는 화면에 떠 있는 동안에만 살아 있고 한 번 쓰면 사라진다. 그래서 화면을 벗어나면 코드도 거둔다.
 * 반대로 한 번 연결한 기기는 계속 쓸 수 있어야 하므로, 여기서 직접 끊기 전까지는 목록에 남는다.
 */

/** 남은 시간을 1초마다 다시 그린다 */
const COUNTDOWN_TICK_MS = 1_000;

export default function MobilePairingSettings() {
  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [remainingSeconds, setRemainingSeconds] = useState(0);
  const [pairedDevices, setPairedDevices] = useState<PairedMobileDevice[]>([]);
  const [failureMessage, setFailureMessage] = useState<string | null>(null);

  /**
   * 발급해 둔 코드가 있는지. 화면을 떠날 때 거둘 것이 있는지 판단하는 데만 쓴다.
   * 정리 함수는 마운트 시점의 상태만 보므로 state로는 지금 값을 알 수 없다.
   */
  const hasOutstandingCode = useRef(false);

  /**
   * 기기 목록을 못 읽어도 설정 화면 전체가 깨지면 안 되므로 빈 목록으로 둔다.
   * 사용자가 누른 동작의 실패는 [failureMessage]로 드러내지만, 화면을 열자마자 도는 이 조회는 보여 줄 것이 없다.
   */
  const refreshPairedDevices = useCallback(async () => {
    try {
      setPairedDevices(await listPairedMobileDevices());
    } catch {
      setPairedDevices([]);
    }
  }, []);

  /** 사용자가 누른 동작. 실패하면 무엇이 안 됐는지 화면에 남긴다 */
  const runPairingAction = async (action: () => Promise<void>, failureText: string) => {
    try {
      setFailureMessage(null);
      await action();
    } catch {
      setFailureMessage(failureText);
    }
  };

  useEffect(() => {
    void refreshPairedDevices();
  }, [refreshPairedDevices]);

  /**
   * 설정 화면을 떠나면 코드를 거둔다. 화면에 없는 코드가 살아 있으면 안 된다.
   * 발급한 적이 없으면 부르지 않는다. 거둘 것이 없는데 부르면 설정 화면을 닫을 때마다 헛된 왕복이 생긴다.
   */
  useEffect(() => {
    return () => {
      if (!hasOutstandingCode.current) {
        return;
      }
      hasOutstandingCode.current = false;

      /** 화면이 이미 사라지는 중이라 실패를 보여 줄 곳이 없다 */
      try {
        void stopMobilePairing().catch(() => {});
      } catch {
        /** 데스크탑 통로가 없는 경우 */
      }
    };
  }, []);

  useEffect(() => {
    if (expiresAt === null) {
      return;
    }

    const updateRemaining = () => {
      const remaining = Math.max(0, Math.ceil((expiresAt - Date.now()) / 1000));
      setRemainingSeconds(remaining);

      if (remaining === 0) {
        /** 만료된 코드는 데스크탑에서도 이미 죽어 있어 거둘 것이 없다 */
        hasOutstandingCode.current = false;
        setPairingCode(null);
        setExpiresAt(null);
      }
    };

    updateRemaining();
    const timer = setInterval(updateRemaining, COUNTDOWN_TICK_MS);
    return () => clearInterval(timer);
  }, [expiresAt]);

  const issueCode = () =>
    runPairingAction(async () => {
      const pairing = await startMobilePairing();
      hasOutstandingCode.current = true;
      setPairingCode(pairing.code);
      setExpiresAt(pairing.expiresAt);
      await refreshPairedDevices();
    }, "연결 코드를 만들지 못했습니다.");

  const cancelCode = () =>
    runPairingAction(async () => {
      await stopMobilePairing();
      hasOutstandingCode.current = false;
      setPairingCode(null);
      setExpiresAt(null);
    }, "코드를 지우지 못했습니다.");

  const disconnectDevice = (deviceId: string) =>
    runPairingAction(async () => {
      await unpairMobileDevice(deviceId);
      await refreshPairedDevices();
    }, "연결을 끊지 못했습니다.");

  return (
    <div id="mobile" className="p-4 border-b border-border-default">
      <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">모바일 연결</h3>

      <div className="flex items-start justify-between gap-4">
        <div>
          <span className="text-sm text-text-primary">모바일 앱 연결</span>
          <p className="text-xs text-text-muted mt-0.5">
            KanVibe 모바일에서 이 데스크탑의 주소와 아래 코드를 입력하면 연결됩니다. 한 번 연결하면 계속 유지됩니다.
          </p>
        </div>

        {pairingCode === null ? (
          <button
            type="button"
            onClick={() => void issueCode()}
            className="shrink-0 px-3 py-1.5 text-sm rounded-md bg-brand-primary text-white hover:bg-brand-hover transition-colors"
          >
            연결 코드 만들기
          </button>
        ) : (
          <button
            type="button"
            onClick={() => void cancelCode()}
            className="shrink-0 px-3 py-1.5 text-sm rounded-md bg-button-neutral text-white hover:bg-button-neutral-hover transition-colors"
          >
            코드 지우기
          </button>
        )}
      </div>

      {pairingCode !== null && (
        <div className="mt-3 p-3 rounded-md bg-bg-page border border-border-default flex items-center justify-between">
          <span className="text-2xl font-mono tracking-[0.3em] text-text-primary">{pairingCode}</span>
          <span className="text-xs text-text-muted">{remainingSeconds}초 뒤 만료</span>
        </div>
      )}

      {failureMessage !== null && (
        <p className="mt-2 text-xs text-status-error">{failureMessage}</p>
      )}

      {pairedDevices.length > 0 && (
        <ul className="mt-3 space-y-1">
          {pairedDevices.map((device) => (
            <li
              key={device.deviceId}
              className="flex items-center justify-between px-3 py-2 rounded-md bg-bg-page border border-border-default"
            >
              <div>
                <span className="text-sm text-text-primary">{device.deviceName}</span>
                <p className="text-xs text-text-muted mt-0.5">
                  {new Date(device.pairedAt).toLocaleString()} 연결됨
                </p>
              </div>
              <button
                type="button"
                onClick={() => void disconnectDevice(device.deviceId)}
                className="px-2 py-1 text-xs rounded-md text-status-error hover:bg-button-neutral-subtle transition-colors"
              >
                연결 끊기
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
