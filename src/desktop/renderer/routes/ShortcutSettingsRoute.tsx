import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslations } from "next-intl";
import { Link } from "@/desktop/renderer/navigation";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import {
  captureShortcutFromEvent,
  formatShortcutForDisplay,
  getCurrentShortcutPlatform,
} from "@/desktop/renderer/utils/keyboardShortcut";
import { saveShortcutBindings, useShortcutBindings } from "@/desktop/renderer/utils/shortcutBindings";
import {
  DEFAULT_SHORTCUT_BINDINGS,
  SHORTCUT_COMMAND_DEFINITIONS,
  findShortcutCommandConflict,
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
  const commandGroups = useMemo(groupShortcutCommands, []);

  useEffect(() => {
    document.title = "Shortcuts";
  }, []);

  const describeCommand = useCallback((definition: ShortcutCommandDefinition) => (
    t(`commands.${definition.labelKey}`, { index: definition.labelIndex ?? 0 })
  ), [t]);

  const applyShortcut = useCallback(async (commandId: ShortcutCommandId, shortcut: string) => {
    await saveShortcutBindings({ ...bindings, [commandId]: shortcut });
  }, [bindings]);

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

    function handleRecordingKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();

      if (event.key === "Escape") {
        setRecordingCommandId(null);
        return;
      }

      const capturedShortcut = captureShortcutFromEvent(event, shortcutPlatform);
      if (!capturedShortcut) {
        setErrorMessage(t("unsupported"));
        return;
      }

      const conflictingCommandId = findShortcutCommandConflict(bindings, targetCommandId, capturedShortcut);
      const conflictingCommand = SHORTCUT_COMMAND_DEFINITIONS
        .find((definition) => definition.id === conflictingCommandId);
      if (conflictingCommand) {
        setErrorMessage(t("conflict", { command: describeCommand(conflictingCommand) }));
        return;
      }

      setErrorMessage(null);
      setRecordingCommandId(null);
      void applyShortcut(targetCommandId, capturedShortcut);
    }

    window.addEventListener("keydown", handleRecordingKeyDown, { capture: true });
    return () => {
      window.removeEventListener("keydown", handleRecordingKeyDown, { capture: true });
      releaseShortcutBlocker();
    };
  }, [applyShortcut, bindings, boardCommands, describeCommand, recordingCommandId, shortcutPlatform, t]);

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
            onClick={() => {
              setErrorMessage(null);
              setRecordingCommandId(null);
              void saveShortcutBindings({ ...DEFAULT_SHORTCUT_BINDINGS });
            }}
            className="shrink-0 rounded-md border border-border-default bg-button-neutral px-3 py-2 text-xs text-text-primary transition-colors hover:border-brand-primary"
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
                        onClick={() => {
                          setErrorMessage(null);
                          setRecordingCommandId(isRecording ? null : definition.id);
                        }}
                        className={`rounded-md border px-3 py-1 text-xs transition-colors ${
                          isRecording
                            ? "border-brand-primary bg-brand-subtle text-brand-primary"
                            : "border-border-default bg-button-neutral text-text-primary hover:border-brand-primary"
                        }`}
                      >
                        {isRecording ? t("recording") : t("record")}
                      </button>
                      <button
                        type="button"
                        disabled={!isCustomized}
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
