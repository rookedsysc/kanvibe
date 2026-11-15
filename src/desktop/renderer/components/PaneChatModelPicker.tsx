import { useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { changePaneChatModelSetting, getPaneChatModelSettings } from "@/desktop/renderer/actions/paneChat";
import type { PaneChatModelSettings, PaneChatModelSettingChange } from "@/lib/aiSessions/paneChatModelSettings";

/** CLI 화면을 열지 않고, 같은 대화의 모델 API를 사용하는 선택 패널이다. */
export function PaneChatModelPicker({ taskId, targetToken, onClose, onChanged }: { taskId: string; targetToken: string; onClose: () => void; onChanged: () => void }) {
  const t = useTranslations("taskDetail.paneChat");
  const [settings, setSettings] = useState<PaneChatModelSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [retry, setRetry] = useState(0);
  const current = useRef(targetToken);
  current.current = targetToken;
  useEffect(() => {
    let cancelled = false;
    setSettings(null);
    setError(false);
    getPaneChatModelSettings(taskId, targetToken).then((result) => { if (!cancelled) setSettings(result); }).catch(() => { if (!cancelled) setError(true); });
    return () => { cancelled = true; };
  }, [taskId, targetToken, retry]);
  async function change(value: PaneChatModelSettingChange) {
    setBusy(true);
    setError(false);
    try {
      await changePaneChatModelSetting(taskId, targetToken, value);
      const result = await getPaneChatModelSettings(taskId, targetToken);
      if (current.current === targetToken) { setSettings(result); onChanged(); }
    } catch { if (current.current === targetToken) setError(true); }
    finally { if (current.current === targetToken) setBusy(false); }
  }
  const model = settings?.models.find((item) => item.id === settings.model);
  const selectClass = "min-h-11 min-w-0 rounded-md border border-border-default bg-bg-page px-3 text-sm text-text-primary focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-50";
  return <section role="dialog" aria-label={t("model")} className="shrink-0 border-b border-border-default bg-bg-surface px-4 py-3" onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); onClose(); } }}>
    <div className="flex flex-wrap items-center gap-3">
      <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-text-secondary">{t("model")}<select autoFocus aria-label={t("model")} className={selectClass} disabled={!settings || busy} value={settings?.model ?? ""} onChange={(event) => void change({ kind: "model", model: event.target.value })}>
        {!model ? <option value={settings?.model ?? ""}>{settings?.model ?? t("loading")}</option> : null}
        {settings?.models.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
      </select></label>
      <label className="flex min-w-0 flex-col gap-1 text-xs text-text-secondary">{t("effort")}<select aria-label={t("effort")} className={selectClass} disabled={!model?.effortLevels.length || busy} value={settings?.effort ?? ""} onChange={(event) => void change({ kind: "effort", effort: event.target.value })}>
        {!model?.effortLevels.some((item) => item.id === settings?.effort) ? <option value={settings?.effort ?? ""}>{settings?.effort ?? t("defaultEffort")}</option> : null}
        {model?.effortLevels.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
      </select></label>
      <button type="button" onClick={onClose} className="min-h-11 rounded px-3 text-xs text-text-secondary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary">{t("closeModel")}</button>
    </div>
    <p className="mt-2 text-xs text-text-secondary">{t("modelAppliesNext")}</p>
    {error ? <div role="alert" className="mt-2 flex items-center gap-2 text-xs text-status-error">{t("modelError")}<button type="button" onClick={() => setRetry((value) => value + 1)} className="min-h-10 px-2">{t("retry")}</button></div> : null}
  </section>;
}
