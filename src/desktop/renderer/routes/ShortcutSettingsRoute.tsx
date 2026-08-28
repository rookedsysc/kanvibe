import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslations } from "next-intl";
import { Link } from "@/desktop/renderer/navigation";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import {
  captureShortcutFromEvent,
  formatShortcutForDisplay,
  getCurrentShortcutPlatform,
  isShortcutModifierKey,
} from "@/desktop/renderer/utils/keyboardShortcut";
import {
  hasLoadedShortcutBindings,
  loadShortcutBindings,
  saveShortcutBindings,
  setShortcutCaptureActive,
  useShortcutBindings,
} from "@/desktop/renderer/utils/shortcutBindings";
import {
  DEFAULT_SHORTCUT_BINDINGS,
  SHORTCUT_COMMAND_DEFINITIONS,
  findShortcutCommandConflict,
  type ShortcutBindings,
  type ShortcutCommandDefinition,
  type ShortcutCommandGroup,
  type ShortcutCommandId,
} from "@/desktop/shared/shortcutBindings";

const SHORTCUT_GROUP_ORDER: ShortcutCommandGroup[] = ["taskDetailDock", "taskDetail", "board", "terminal"];

function groupShortcutCommands(): Array<{ group: ShortcutCommandGroup; commands: ShortcutCommandDefinition[] }> {
  return SHORTCUT_GROUP_ORDER.map((group) => ({
    group,
    commands: SHORTCUT_COMMAND_DEFINITIONS.filter((definition) => definition.group === group),
  }));
}

export default function ShortcutSettingsRoute() {
  const t = useTranslations("settings.shortcuts");
  const bindings = useShortcutBindings();
  const boardCommands = useBoardCommands();
  const shortcutPlatform = getCurrentShortcutPlatform();
  const [recordingCommandId, setRecordingCommandId] = useState<ShortcutCommandId | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isBindingsLoaded, setIsBindingsLoaded] = useState(hasLoadedShortcutBindings);
  const commandGroups = useMemo(groupShortcutCommands, []);

  useEffect(() => {
    document.title = t("title");
  }, [t]);

  /**
   * 앱이 뜰 때의 조회 한 번이 실패하면 이 화면은 재배정이 전부 사라진 것처럼 보인다.
   * 그 위에서 저장하면 표가 기본값으로 치환돼 실제로도 사라지므로, 담긴 표가 없을 때만 한 번 더 읽는다.
   * 이미 담겨 있는데도 다시 읽으면, 늦게 도착한 응답이 그 사이에 저장한 값을 되돌린다.
   */
  useEffect(() => {
    if (hasLoadedShortcutBindings()) {
      return;
    }

    void loadShortcutBindings().then((isLoaded) => {
      setIsBindingsLoaded(isLoaded);
      if (!isLoaded) {
        setErrorMessage(t("loadFailed"));
      }
    });
  }, [t]);

  const describeCommand = useCallback((definition: ShortcutCommandDefinition) => (
    t(`commands.${definition.labelKey}`, { index: definition.labelIndex ?? 0 })
  ), [t]);

  /** 저장이 실패하면 화면은 옛 값 그대로다. 조용히 넘기면 사용자는 바뀐 줄 안다 */
  const persistBindings = useCallback(async (nextBindings: ShortcutBindings) => {
    /** 저장은 표를 통째로 치환한다. 조회하지 못한 기본값 표를 저장하면 저장돼 있던 재배정이 전부 지워진다 */
    if (!hasLoadedShortcutBindings()) {
      setIsBindingsLoaded(false);
      setErrorMessage(t("loadFailed"));
      return;
    }

    try {
      await saveShortcutBindings(nextBindings);
    } catch (error) {
      console.error("단축키 저장 실패:", error);
      setErrorMessage(t("saveFailed"));
    }
  }, [t]);

  /** 겹치는 조합이 붙으면 정의 순서상 뒤인 명령은 렌더러와 main 양쪽에서 도달할 방법이 없어진다 */
  const describeShortcutConflict = useCallback((commandId: ShortcutCommandId, shortcut: string) => {
    const conflictingCommandId = findShortcutCommandConflict(bindings, commandId, shortcut, shortcutPlatform);
    const conflictingCommand = SHORTCUT_COMMAND_DEFINITIONS
      .find((definition) => definition.id === conflictingCommandId);
    return conflictingCommand ? t("conflict", { command: describeCommand(conflictingCommand) }) : null;
  }, [bindings, describeCommand, shortcutPlatform, t]);

  /** 녹화든 개별 되돌리기든 같은 관문을 지나야 중복 배정이 조용히 생기지 않는다 */
  const applyShortcut = useCallback(async (commandId: ShortcutCommandId, shortcut: string) => {
    const conflictMessage = describeShortcutConflict(commandId, shortcut);
    if (conflictMessage) {
      setErrorMessage(conflictMessage);
      return;
    }

    await persistBindings({ ...bindings, [commandId]: shortcut });
  }, [bindings, describeShortcutConflict, persistBindings]);

  /**
   * 녹화 중에는 앱의 다른 단축키 처리를 멈춘다.
   * 그러지 않으면 새 조합을 누르는 순간 그 조합의 원래 명령이 함께 실행된다.
   */
  useEffect(() => {
    if (!recordingCommandId) {
      return;
    }

    const targetCommandId = recordingCommandId;
    const releaseShortcutBlocker = boardCommands.registerShortcutBlocker();
    /** Electron main도 녹화 중임을 알아야 한다. 모르면 main이 가로채는 조합은 여기까지 오지 못한다 */
    setShortcutCaptureActive(true);

    function handleRecordingKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();

      if (event.key === "Escape") {
        setErrorMessage(null);
        setRecordingCommandId(null);
        return;
      }

      /** 조합을 누르는 모든 사용자가 수식키 keydown을 먼저 흘린다. 이걸 오류로 보면 매 녹화마다 거짓 배너가 뜬다 */
      if (isShortcutModifierKey(event.key)) {
        return;
      }

      const capturedShortcut = captureShortcutFromEvent(event, shortcutPlatform);
      if (!capturedShortcut) {
        setErrorMessage(t("unsupported"));
        return;
      }

      const conflictMessage = describeShortcutConflict(targetCommandId, capturedShortcut);
      if (conflictMessage) {
        setErrorMessage(conflictMessage);
        return;
      }

      setErrorMessage(null);
      setRecordingCommandId(null);
      void applyShortcut(targetCommandId, capturedShortcut);
    }

    window.addEventListener("keydown", handleRecordingKeyDown, { capture: true });
    return () => {
      window.removeEventListener("keydown", handleRecordingKeyDown, { capture: true });
      setShortcutCaptureActive(false);
      releaseShortcutBlocker();
    };
  }, [applyShortcut, boardCommands, describeShortcutConflict, recordingCommandId, shortcutPlatform, t]);

  return (
    <div data-shortcut-capture="true" className="min-h-screen bg-bg-page px-6 py-8">
      <div className="mx-auto w-full max-w-3xl">
        <Link href="/settings" className="mb-8 inline-flex items-center gap-3 text-xs font-medium text-text-muted hover:text-text-primary">
          <span aria-hidden="true">←</span>
          {t("title")}
        </Link>

        <header className="mb-6 flex items-start justify-between gap-4">
          <div>
            <h1 className="text-lg font-semibold text-text-primary">{t("title")}</h1>
            <p className="mt-1 text-sm text-text-muted">{t("pageDescription")}</p>
            <p className="mt-1 text-xs text-text-muted">{t("recordHint")}</p>
          </div>
          <button
            type="button"
            disabled={!isBindingsLoaded}
            onClick={() => {
              setErrorMessage(null);
              setRecordingCommandId(null);
              void persistBindings({ ...DEFAULT_SHORTCUT_BINDINGS });
            }}
            className="shrink-0 rounded-md border border-border-default bg-button-neutral px-3 py-2 text-xs text-text-primary transition-colors hover:border-brand-primary disabled:cursor-not-allowed disabled:opacity-40"
          >
            {t("resetAll")}
          </button>
        </header>

        {errorMessage ? (
          <p role="alert" className="mb-4 rounded-md border border-status-warning/20 bg-status-warning/15 px-3 py-2 text-xs text-status-warning">
            {errorMessage}
          </p>
        ) : null}

        {commandGroups.map(({ group, commands }) => (
          <section key={group} className="mb-6 rounded-lg border border-border-default bg-bg-surface">
            <h2 className="border-b border-border-default px-4 py-3 text-xs uppercase tracking-wide text-text-muted">
              {t(`groups.${group}`)}
            </h2>
            <ul>
              {commands.map((definition) => {
                const isRecording = recordingCommandId === definition.id;
                const isCustomized = bindings[definition.id] !== definition.defaultShortcut;

                return (
                  <li key={definition.id} className="flex items-center justify-between gap-3 border-b border-border-default px-4 py-3 last:border-b-0">
                    <span className="text-sm text-text-primary">{describeCommand(definition)}</span>
                    <div className="flex items-center gap-2">
                      <code className="rounded-md border border-border-default bg-bg-page px-2 py-1 text-xs text-text-secondary">
                        {formatShortcutForDisplay(bindings[definition.id], shortcutPlatform)}
                      </code>
                      <button
                        type="button"
                        disabled={!isBindingsLoaded}
                        onClick={() => {
                          setErrorMessage(null);
                          setRecordingCommandId(isRecording ? null : definition.id);
                        }}
                        className={`rounded-md border px-3 py-1 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                          isRecording
                            ? "border-brand-primary bg-brand-subtle text-brand-primary"
                            : "border-border-default bg-button-neutral text-text-primary hover:border-brand-primary"
                        }`}
                      >
                        {isRecording ? t("recording") : t("record")}
                      </button>
                      <button
                        type="button"
                        disabled={!isCustomized || !isBindingsLoaded}
                        onClick={() => {
                          setErrorMessage(null);
                          void applyShortcut(definition.id, definition.defaultShortcut);
                        }}
                        className="rounded-md border border-border-default px-3 py-1 text-xs text-text-muted transition-colors hover:border-brand-primary hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-40"
                      >
                        {t("reset")}
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
