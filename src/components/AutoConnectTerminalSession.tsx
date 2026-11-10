"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { connectTerminalSession } from "@/desktop/renderer/actions/kanban";
import { ensureSessionDependencyWithPrompt } from "@/desktop/renderer/utils/sessionDependencyPrompt";
import type { KanbanTask } from "@/entities/KanbanTask";
import { SessionType } from "@/entities/KanbanTask";

interface AutoConnectTerminalSessionProps {
  taskId: string;
  sshHost?: string | null;
  defaultSessionType?: SessionType;
  onConnected?: (task: KanbanTask) => void;
}

/** 의존성 안내를 사용자가 거절한 경우는 실패가 아니므로 오류 문구 없이 연결 버튼만 남긴다 */
type SessionConnectOutcome =
  | { kind: "connected"; task: KanbanTask }
  | { kind: "declined" }
  | { kind: "failed"; message: string | null };

type AutoConnectState =
  | { kind: "connecting" }
  | { kind: "declined" }
  | { kind: "failed"; message: string | null };

/**
 * 같은 task 연결 요청이 겹치면(StrictMode의 effect 재실행, 빠른 재선택) 진행 중인 요청을 함께 기다린다.
 * 그러지 않으면 의존성 안내가 두 번 뜨고 세션도 두 번 만들려고 한다.
 */
const pendingSessionConnects = new Map<string, Promise<SessionConnectOutcome>>();

function connectTaskSessionOnce(
  taskId: string,
  sessionType: SessionType,
  sshHost: string | null | undefined,
  commonTranslations: ReturnType<typeof useTranslations>,
): Promise<SessionConnectOutcome> {
  const pendingConnect = pendingSessionConnects.get(taskId);
  if (pendingConnect) {
    return pendingConnect;
  }

  const connect = (async (): Promise<SessionConnectOutcome> => {
    try {
      const isDependencyReady = await ensureSessionDependencyWithPrompt(sessionType, sshHost, commonTranslations);
      if (!isDependencyReady) {
        return { kind: "declined" };
      }

      const connectedTask = await connectTerminalSession(taskId, sessionType);
      return connectedTask ? { kind: "connected", task: connectedTask } : { kind: "failed", message: null };
    } catch (error) {
      return { kind: "failed", message: error instanceof Error ? error.message : null };
    }
  })().finally(() => {
    pendingSessionConnects.delete(taskId);
  });

  pendingSessionConnects.set(taskId, connect);
  return connect;
}

/** 세션이 없는 태스크를 보여 주는 순간 기본 세션 타입으로 터미널 세션을 연결한다 */
export default function AutoConnectTerminalSession({
  taskId,
  sshHost,
  defaultSessionType = SessionType.HERDR,
  onConnected,
}: AutoConnectTerminalSessionProps) {
  const t = useTranslations("taskDetail");
  const tc = useTranslations("common");
  const [connectAttempt, setConnectAttempt] = useState(0);
  const [connectState, setConnectState] = useState<AutoConnectState>({ kind: "connecting" });

  useEffect(() => {
    let cancelled = false;
    setConnectState({ kind: "connecting" });

    void connectTaskSessionOnce(taskId, defaultSessionType, sshHost, tc).then((outcome) => {
      if (cancelled) {
        return;
      }

      if (outcome.kind === "connected") {
        onConnected?.(outcome.task);
        return;
      }

      setConnectState(outcome);
    });

    return () => {
      cancelled = true;
    };
    // 번역 함수와 콜백은 렌더마다 새로 만들어질 수 있어, 연결 시도를 task·세션 타입·재시도로만 다시 시작한다
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [taskId, defaultSessionType, sshHost, connectAttempt]);

  if (connectState.kind === "connecting") {
    return (
      <p role="status" className="text-sm text-text-muted">
        {t("connecting")}
      </p>
    );
  }

  const failureMessage = connectState.kind === "failed" ? connectState.message ?? t("connectFailed") : null;

  return (
    <div className="flex flex-col items-center gap-3">
      <p className="text-sm text-text-muted">{t("noTerminal")}</p>
      {failureMessage ? <p role="alert" className="text-xs text-status-error">{failureMessage}</p> : null}
      <button
        type="button"
        onClick={() => setConnectAttempt((attempt) => attempt + 1)}
        className="rounded-md bg-brand-primary px-4 py-1.5 text-sm font-medium text-text-inverse transition-colors hover:bg-brand-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-primary focus-visible:ring-offset-2"
      >
        {t("connectTerminal")}
      </button>
    </div>
  );
}
