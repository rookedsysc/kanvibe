"use client";

import { useEffect, useSyncExternalStore } from "react";
import { useTranslations } from "next-intl";

/** 진행은 끝날 때까지, 성공은 잠시 뒤 사라지고, 실패는 사용자가 닫을 때까지 남는다 */
export type ActionStatusKind = "progress" | "success" | "failure";

export interface ActionStatusInput {
  kind: ActionStatusKind;
  message: string;
  /** 결과에서 바로 이어 할 일. 누르면 실행하고 알림을 닫는다 */
  action?: { label: string; onAction: () => void };
}

interface ActionStatusEntry extends ActionStatusInput {
  id: string;
}

const SUCCESS_STATUS_DURATION_MS = 5_000;

let actionStatusEntries: ActionStatusEntry[] = [];
let nextActionStatusId = 0;
const actionStatusListeners = new Set<() => void>();

function publishActionStatusEntries(nextEntries: ActionStatusEntry[]) {
  actionStatusEntries = nextEntries;
  for (const listener of actionStatusListeners) {
    listener();
  }
}

function subscribeActionStatus(listener: () => void) {
  actionStatusListeners.add(listener);
  return () => {
    actionStatusListeners.delete(listener);
  };
}

function getActionStatusEntries() {
  return actionStatusEntries;
}

/**
 * 화면을 닫은 뒤에도 끝난 작업의 결과를 알린다. 모달·팔레트처럼 결과를 보일 자리가 사라진 작업이 쓴다.
 * 돌려준 id로 진행 알림을 닫고 결과 알림으로 바꿀 수 있다.
 */
export function showActionStatus(input: ActionStatusInput): string {
  nextActionStatusId += 1;
  const id = `action-status-${nextActionStatusId}`;
  publishActionStatusEntries([...actionStatusEntries, { ...input, id }]);
  return id;
}

export function dismissActionStatus(id: string) {
  publishActionStatusEntries(actionStatusEntries.filter((entry) => entry.id !== id));
}

function ActionStatusItem({ entry }: { entry: ActionStatusEntry }) {
  const tc = useTranslations("common");
  const isFailure = entry.kind === "failure";

  useEffect(() => {
    if (entry.kind !== "success") {
      return;
    }

    const timeoutId = window.setTimeout(() => dismissActionStatus(entry.id), SUCCESS_STATUS_DURATION_MS);
    return () => window.clearTimeout(timeoutId);
  }, [entry.id, entry.kind]);

  return (
    <div
      role={isFailure ? "alert" : "status"}
      className={`pointer-events-auto flex items-start gap-3 rounded-lg border bg-bg-surface px-3 py-2 text-sm shadow-md ${
        isFailure ? "border-status-error/30 text-status-error" : "border-border-default text-text-primary"
      }`}
    >
      {entry.kind === "progress" ? (
        <span aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0 rounded-full border-2 border-current border-t-transparent motion-safe:animate-spin" />
      ) : null}
      <span className="min-w-0 flex-1 break-words">{entry.message}</span>
      {entry.action ? (
        <button
          type="button"
          onClick={() => {
            dismissActionStatus(entry.id);
            entry.action?.onAction();
          }}
          className="shrink-0 rounded px-1.5 py-0.5 text-xs font-medium text-text-brand hover:underline focus-visible:outline focus-visible:outline-1 focus-visible:outline-border-brand"
        >
          {entry.action.label}
        </button>
      ) : null}
      <button
        type="button"
        onClick={() => dismissActionStatus(entry.id)}
        aria-label={tc("close")}
        className="shrink-0 rounded px-1 text-xs text-text-muted hover:text-text-primary focus-visible:outline focus-visible:outline-1 focus-visible:outline-border-brand"
      >
        ×
      </button>
    </div>
  );
}

/** App에 한 번만 둔다. 어디서 보낸 결과든 화면 오른쪽 아래에 쌓아 보인다 */
export default function ActionStatusToast() {
  const entries = useSyncExternalStore(subscribeActionStatus, getActionStatusEntries, getActionStatusEntries);

  if (entries.length === 0) {
    return null;
  }

  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[600] flex w-[min(24rem,calc(100%-2rem))] flex-col gap-2">
      {entries.map((entry) => (
        <ActionStatusItem key={entry.id} entry={entry} />
      ))}
    </div>
  );
}
