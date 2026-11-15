import { useEffect, useState } from "react";
import { getAppSetting, setAppSetting } from "@/desktop/renderer/actions/appSettings";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";

export const PANE_CHAT_DEFAULT_KEY = "pane_chat_default_enabled";
export const PANE_CHAT_INSTRUCTIONS_KEY = "pane_chat_show_instructions";

/** 채팅 표시 설정을 앱 설정에 보존하고 설정 화면의 갱신도 반영한다. */
export function usePaneChatPreferences() {
  const refresh = useRefreshSignal(["settings"]);
  const [preferences, setPreferences] = useState({ defaultEnabled: false, showInstructions: false, loaded: false });
  const [error, setError] = useState(false);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [enabled, instructions] = await Promise.all([getAppSetting(PANE_CHAT_DEFAULT_KEY), getAppSetting(PANE_CHAT_INSTRUCTIONS_KEY)]);
        if (!cancelled) {
          setPreferences({ defaultEnabled: enabled === "true", showInstructions: instructions === "true", loaded: true });
          setError(false);
        }
      } catch { if (!cancelled) setError(true); }
    })();
    return () => { cancelled = true; };
  }, [refresh]);

  async function update(key: string, value: boolean) {
    setSaving(true);
    setError(false);
    try {
      await setAppSetting(key, String(value));
      setPreferences((current) => ({ ...current, [key === PANE_CHAT_DEFAULT_KEY ? "defaultEnabled" : "showInstructions"]: value }));
    } catch { setError(true); }
    finally { setSaving(false); }
  }
  return { ...preferences, error, saving, setDefaultEnabled: (value: boolean) => update(PANE_CHAT_DEFAULT_KEY, value), setShowInstructions: (value: boolean) => update(PANE_CHAT_INSTRUCTIONS_KEY, value) };
}
