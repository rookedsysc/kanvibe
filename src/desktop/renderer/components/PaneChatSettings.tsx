import { useTranslations } from "next-intl";
import { usePaneChatPreferences } from "@/desktop/renderer/hooks/usePaneChatPreferences";

/** 설정 화면에서 새 pane의 첫 표시 모드를 선택한다. 기존 pane의 명시적 선택은 유지한다. */
export function PaneChatSettings() {
  const t = useTranslations("settings");
  const preferences = usePaneChatPreferences();
  return <div className="mt-4 border-t border-border-subtle pt-4">
    <label className="flex min-h-11 items-center justify-between gap-4">
      <span><span className="text-sm text-text-primary">{t("defaultChatMode")}</span><span className="mt-0.5 block text-xs text-text-muted">{t("defaultChatModeDescription")}</span></span>
      <input type="checkbox" role="switch" aria-label={t("defaultChatMode")} checked={preferences.defaultEnabled} disabled={!preferences.loaded || preferences.saving} onChange={(event) => void preferences.setDefaultEnabled(event.target.checked)} className="h-5 w-5 shrink-0 accent-brand-primary focus-visible:outline-2 focus-visible:outline-brand-primary" />
    </label>
    {preferences.error ? <p role="alert" className="mt-2 text-xs text-status-error">{t("chatSettingsError")}</p> : null}
  </div>;
}
