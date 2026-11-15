import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslations } from "next-intl";
import { changePaneChatModelSetting, getPaneChatModelSettings } from "@/desktop/renderer/actions/paneChat";
import type { PaneChatModelSettings, PaneChatModelSettingChange } from "@/lib/aiSessions/paneChatModelSettings";

export function PaneChatModelPicker({ taskId, targetToken, initialSettings, onClose, onChanged }: { taskId: string; targetToken: string; initialSettings?: PaneChatModelSettings; onClose: (restoreFocus?: boolean) => void; onChanged: () => void }) {
  const t = useTranslations("taskDetail.paneChat");
  const [settings, setSettings] = useState<PaneChatModelSettings | null>(initialSettings ?? null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [retry, setRetry] = useState(0);
  const dialog = useRef<HTMLElement>(null);
  const generation = useRef(0);
  const changing = useRef(false);
  const close = useRef(onClose);
  close.current = onClose;
  const hasSettings = settings !== null;
  useEffect(() => {
    let cancelled = false;
    const epoch = ++generation.current;
    setError(false);
    getPaneChatModelSettings(taskId, targetToken).then((result) => {
      if (!cancelled && generation.current === epoch) setSettings(result);
    }).catch(() => { if (!cancelled && generation.current === epoch) setError(true); });
    const invalidatePendingUpdates = () => { generation.current++; };
    return () => { cancelled = true; invalidatePendingUpdates(); };
  }, [taskId, targetToken, retry]);
  useEffect(() => {
    if (hasSettings) (dialog.current?.querySelector<HTMLButtonElement>('[aria-checked="true"]') ?? dialog.current?.querySelector<HTMLButtonElement>('[role="menuitemradio"]'))?.focus();
    else dialog.current?.focus();
  }, [hasSettings]);
  useEffect(() => {
    function dismiss(event: PointerEvent) {
      const target = event.target;
      if (target instanceof Element && !dialog.current?.contains(target) && !target.closest("[data-pane-chat-model-trigger]")) close.current(false);
    }
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, []);
  async function change(value: PaneChatModelSettingChange) {
    if (changing.current) return;
    const epoch = ++generation.current;
    changing.current = true;
    setBusy(true);
    setError(false);
    try {
      await changePaneChatModelSetting(taskId, targetToken, value);
      if (generation.current !== epoch) return;
      const result = await getPaneChatModelSettings(taskId, targetToken);
      if (generation.current === epoch) { setSettings(result); onChanged(); }
    } catch { if (generation.current === epoch) setError(true); }
    finally {
      changing.current = false;
      if (generation.current === epoch) setBusy(false);
    }
  }
  function navigateModels(event: KeyboardEvent) {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const options = Array.from(dialog.current?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled)') ?? []);
    if (!options.length) return;
    event.preventDefault();
    const index = options.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length;
    options[next]?.focus();
  }
  const model = settings?.models.find((item) => item.id === settings.model);
  const optionClass = "min-h-9 rounded-md px-2.5 text-xs focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40";
  return <section ref={dialog} role="dialog" tabIndex={-1} aria-label={t("model")} aria-busy={busy} className="max-h-[min(32rem,65vh)] overflow-y-auto rounded-xl border border-border-default bg-bg-surface text-text-primary shadow-xl outline-none" onKeyDown={(event) => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
  }}>
    <header className="flex items-center justify-between px-4 pb-1 pt-3">
      <h3 className="text-xs font-medium text-text-secondary">{t("model")}</h3>
      <button type="button" aria-label={t("closeModel")} onClick={() => onClose()} className="flex h-8 w-8 items-center justify-center rounded-md text-lg text-text-secondary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary"><span aria-hidden="true">×</span></button>
    </header>
    {!settings ? <p role="status" className="px-4 py-5 text-sm text-text-secondary">{t("loading")}</p> : <>
      <div role="menu" aria-label={t("model")} onKeyDown={navigateModels} className="max-h-[min(35vh,18rem)] overflow-y-auto px-2 pb-2">
        {settings.models.map((item) => <button key={item.id} type="button" role="menuitemradio" aria-checked={item.id === settings.model} disabled={busy} onClick={() => { if (item.id !== settings.model) void change({ kind: "model", model: item.id }); }} className={`flex min-h-11 w-full items-center gap-3 rounded-lg px-3 text-left text-sm focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-50 ${item.id === settings.model ? "bg-brand-subtle text-text-primary" : "hover:bg-bg-page"}`}>
          <span className="min-w-0 flex-1 truncate">{item.label}</span>
          {item.id === settings.model ? <svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0 text-brand-primary"><path d="m3 8 3 3 7-7" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" /></svg> : null}
        </button>)}
      </div>
      {model?.effortLevels.length ? <fieldset disabled={busy} className="border-t border-border-subtle px-4 py-3">
        <legend className="sr-only">{t("effort")}</legend>
        <div className="mb-2 flex items-center justify-between gap-2"><span className="text-xs text-text-secondary">{t("effort")}</span><button type="button" aria-pressed={!model.effortLevels.some((item) => item.id === settings.effort)} onClick={() => void change({ kind: "reset" })} className="rounded px-1 text-xs text-text-secondary hover:text-text-primary focus-visible:outline-2 focus-visible:outline-brand-primary">{t("defaultEffort")}</button></div>
        <div className="flex flex-wrap gap-1 rounded-lg bg-bg-page p-1">{model.effortLevels.map((item) => <button key={item.id} type="button" aria-pressed={item.id === settings.effort} onClick={() => { if (item.id !== settings.effort) void change({ kind: "effort", effort: item.id }); }} className={`${optionClass} flex-1 ${item.id === settings.effort ? "bg-bg-surface font-medium text-text-primary shadow-sm" : "text-text-secondary hover:bg-bg-surface"}`}>{item.label}</button>)}</div>
      </fieldset> : null}
      {model?.supportsFast ? <div className="flex items-center justify-between border-t border-border-subtle px-4 py-3"><span className="text-xs text-text-secondary">{t("fastMode")}</span><button type="button" role="switch" aria-label={t("fastMode")} aria-checked={settings.isFast === true} disabled={busy || settings.isFast === null} onClick={() => void change({ kind: "fast", isFast: !settings.isFast })} className={`relative h-6 w-10 rounded-full focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-primary disabled:opacity-40 ${settings.isFast ? "bg-brand-primary" : "bg-border-default"}`}><span aria-hidden="true" className={`absolute top-1 h-4 w-4 rounded-full bg-white ${settings.isFast ? "left-5" : "left-1"}`} /></button></div> : null}
    </>}
    <p className="border-t border-border-subtle px-4 py-3 text-xs leading-5 text-text-secondary">{busy ? t("savingModel") : t("modelAppliesNext")}</p>
    {error ? <div role="alert" className="flex items-center gap-2 px-4 pb-3 text-xs text-status-error">{t("modelError")}<button type="button" disabled={busy} onClick={() => setRetry((value) => value + 1)} className="min-h-9 shrink-0 rounded px-2 text-text-primary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary">{t("retry")}</button></div> : null}
  </section>;
}
