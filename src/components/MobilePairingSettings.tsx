"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import InlineLoadError from "@/components/InlineLoadError";
import {
  listPairedMobileDevices,
  readMobileConnectionSettings,
  readMobilePairingCode,
  startMobilePairing,
  stopMobilePairing,
  unpairMobileDevice,
  updateLanPlaintextAllowed,
  type MobileConnectionSettings,
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

/** 진행 중인 연결 설정 작업. 기기 해제는 어느 기기인지까지 들고 있어 그 행에만 진행을 보인다 */
type PairingActionTarget = "issue-code" | "clear-code" | "lan-plaintext" | { deviceId: string };

export default function MobilePairingSettings() {
  const t = useTranslations("settings");
  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [remainingSeconds, setRemainingSeconds] = useState(0);
  const [pairedDevices, setPairedDevices] = useState<PairedMobileDevice[]>([]);
  const [failureMessage, setFailureMessage] = useState<string | null>(null);
  const [connectionSettings, setConnectionSettings] = useState<MobileConnectionSettings | null>(null);
  const [pendingAction, setPendingAction] = useState<PairingActionTarget | null>(null);
  /** state는 다음 렌더에야 바뀌므로 같은 순간에 겹쳐 누른 작업은 ref로 막는다 */
  const isActionRunning = useRef(false);
  const [isDeviceListUnreadable, setIsDeviceListUnreadable] = useState(false);

  /**
   * 발급해 둔 코드가 있는지. 화면을 떠날 때 거둘 것이 있는지 판단하는 데만 쓴다.
   * 정리 함수는 마운트 시점의 상태만 보므로 state로는 지금 값을 알 수 없다.
   */
  const hasOutstandingCode = useRef(false);

  /**
   * 기기 목록을 못 읽어도 설정 화면 전체가 깨지면 안 된다. 다만 빈 목록으로 바꾸면 연결한 기기가 없는 것처럼 보이므로,
   * 이전 목록은 그대로 두고 읽지 못했다는 안내와 다시 읽기를 따로 보인다.
   */
  const refreshPairedDevices = useCallback(async () => {
    try {
      setPairedDevices(await listPairedMobileDevices());
      setIsDeviceListUnreadable(false);
    } catch {
      setIsDeviceListUnreadable(true);
    }
  }, []);

  /** 포트 안내와 평문 설정은 부가 정보이므로 못 읽으면 그 부분만 숨긴다 */
  const refreshConnectionSettings = useCallback(async () => {
    try {
      setConnectionSettings(await readMobileConnectionSettings());
    } catch {
      setConnectionSettings(null);
    }
  }, []);

  /**
   * 사용자가 누른 동작. 코드 발급·폐기·기기 해제는 서로 상태를 바꾸므로 한 번에 하나만 보내고,
   * 실패하면 무엇이 안 됐는지 화면에 남긴다.
   */
  const runPairingAction = async (target: PairingActionTarget, action: () => Promise<void>, failureText: string) => {
    if (isActionRunning.current) {
      return;
    }

    isActionRunning.current = true;
    setPendingAction(target);
    try {
      setFailureMessage(null);
      await action();
    } catch {
      setFailureMessage(failureText);
    } finally {
      isActionRunning.current = false;
      setPendingAction(null);
    }
  };
  const isActionPending = pendingAction !== null;

  useEffect(() => {
    void refreshPairedDevices();
    void refreshConnectionSettings();
  }, [refreshPairedDevices, refreshConnectionSettings]);

  const changeLanPlaintextAllowed = (isAllowed: boolean) => runPairingAction("lan-plaintext", async () => {
    await updateLanPlaintextAllowed(isAllowed);
    setConnectionSettings((current) => current && { ...current, isLanPlaintextAllowed: isAllowed });
  }, t("mobileLanPlaintextUpdateFailed"));

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

    const forgetCode = () => {
      /** 데스크탑에서 이미 죽은 코드라 거둘 것이 없다 */
      hasOutstandingCode.current = false;
      setPairingCode(null);
      setExpiresAt(null);
    };

    const updateRemaining = () => {
      const remaining = Math.max(0, Math.ceil((expiresAt - Date.now()) / 1000));
      setRemainingSeconds(remaining);

      if (remaining === 0) {
        forgetCode();
        return;
      }

      /**
       * 만료 말고도 코드가 사라지는 길이 있다. 실패가 상한에 닿으면 데스크탑이 코드를 버리는데,
       * 그것을 여기서 확인하지 않으면 남은 시간이 흐르는 6자리가 화면에 그대로 남아
       * 사용자는 그 코드로 무엇도 연결되지 않는 이유를 찾지 못한다.
       */
      void readMobilePairingCode()
        .then((code) => {
          if (code === null) {
            forgetCode();
          }
        })
        .catch(() => {});
    };

    updateRemaining();
    const timer = setInterval(updateRemaining, COUNTDOWN_TICK_MS);
    return () => clearInterval(timer);
  }, [expiresAt]);

  const issueCode = () =>
    runPairingAction("issue-code", async () => {
      const pairing = await startMobilePairing();
      hasOutstandingCode.current = true;
      setPairingCode(pairing.code);
      setExpiresAt(pairing.expiresAt);
      await refreshPairedDevices();
    }, t("mobilePairingIssueCodeFailed"));

  const cancelCode = () =>
    runPairingAction("clear-code", async () => {
      await stopMobilePairing();
      hasOutstandingCode.current = false;
      setPairingCode(null);
      setExpiresAt(null);
    }, t("mobilePairingClearCodeFailed"));

  const disconnectDevice = (deviceId: string) =>
    runPairingAction({ deviceId }, async () => {
      await unpairMobileDevice(deviceId);
      await refreshPairedDevices();
    }, t("mobilePairingDisconnectFailed"));

  return (
    <div id="mobile" className="p-4 border-b border-border-default">
      <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">{t("mobilePairingSection")}</h3>

      <div className="flex items-start justify-between gap-4">
        <div>
          <span className="text-sm text-text-primary">{t("mobilePairing")}</span>
          <p className="text-xs text-text-muted mt-0.5">
            {t("mobilePairingDescription")}
          </p>
          {connectionSettings && (
            <p className="text-xs text-text-muted mt-0.5">
              {t("mobilePairingPort", { port: connectionSettings.mobileTlsPort })}
            </p>
          )}
        </div>

        {pairingCode === null ? (
          <button
            type="button"
            onClick={() => void issueCode()}
            disabled={isActionPending}
            aria-busy={pendingAction === "issue-code" || undefined}
            className="shrink-0 px-3 py-1.5 text-sm rounded-md bg-brand-primary text-white hover:bg-brand-hover transition-colors disabled:opacity-50"
          >
            {pendingAction === "issue-code" ? t("mobilePairingIssuingCode") : t("mobilePairingIssueCode")}
          </button>
        ) : (
          <button
            type="button"
            onClick={() => void cancelCode()}
            disabled={isActionPending}
            aria-busy={pendingAction === "clear-code" || undefined}
            className="shrink-0 px-3 py-1.5 text-sm rounded-md bg-button-neutral text-white hover:bg-button-neutral-hover transition-colors disabled:opacity-50"
          >
            {pendingAction === "clear-code" ? t("mobilePairingClearingCode") : t("mobilePairingClearCode")}
          </button>
        )}
      </div>

      {pairingCode !== null && (
        <div className="mt-3 p-3 rounded-md bg-bg-page border border-border-default flex items-center justify-between">
          <span className="text-2xl font-mono tracking-[0.3em] text-text-primary">{pairingCode}</span>
          <span className="text-xs text-text-muted">{t("mobilePairingCodeExpiresIn", { seconds: remainingSeconds })}</span>
        </div>
      )}

      {failureMessage !== null && (
        <p role="alert" className="mt-2 text-xs text-status-error">{failureMessage}</p>
      )}

      {isDeviceListUnreadable && (
        <InlineLoadError
          className="mt-3"
          message={t("mobilePairingDevicesLoadFailed")}
          onRetry={() => void refreshPairedDevices()}
        />
      )}

      {pairedDevices.length > 0 && (
        <ul className="mt-3 space-y-1">
          {pairedDevices.map((device) => {
            const isDisconnecting = typeof pendingAction === "object" && pendingAction?.deviceId === device.deviceId;
            return (
              <li
                key={device.deviceId}
                className="flex items-center justify-between px-3 py-2 rounded-md bg-bg-page border border-border-default"
              >
                <div>
                  <span className="text-sm text-text-primary">{device.deviceName}</span>
                  <p className="text-xs text-text-muted mt-0.5">
                    {t("mobilePairingPairedAt", { date: new Date(device.pairedAt).toLocaleString() })}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => void disconnectDevice(device.deviceId)}
                  disabled={isActionPending}
                  aria-busy={isDisconnecting || undefined}
                  className="px-2 py-1 text-xs rounded-md text-status-error hover:bg-button-neutral-subtle transition-colors disabled:opacity-50"
                >
                  {isDisconnecting ? t("mobilePairingDisconnecting") : t("mobilePairingDisconnect")}
                </button>
              </li>
            );
          })}
        </ul>
      )}

      {connectionSettings && (
        <label className="mt-3 flex items-start justify-between gap-3">
          <span>
            <span className="text-sm text-text-primary">{t("mobileLanPlaintextAllow")}</span>
            <span className="block text-xs text-text-muted mt-0.5">{t("mobileLanPlaintextDescription")}</span>
          </span>
          <input
            type="checkbox"
            className="mt-1 shrink-0"
            checked={connectionSettings.isLanPlaintextAllowed}
            disabled={isActionPending}
            onChange={(event) => void changeLanPlaintextAllowed(event.target.checked)}
          />
        </label>
      )}
    </div>
  );
}
