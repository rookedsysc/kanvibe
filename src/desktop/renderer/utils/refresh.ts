import { useEffect, useState } from "react";

const REFRESH_EVENT_NAME = "kanvibe:navigation-refresh";

export type DesktopRefreshScope = "all" | "board" | "task-detail" | "diff" | "pane-layout" | "settings";

interface DesktopRefreshDetail {
  scope: DesktopRefreshScope;
}

export function triggerDesktopRefresh(scope: DesktopRefreshScope = "all") {
  window.dispatchEvent(new CustomEvent<DesktopRefreshDetail>(REFRESH_EVENT_NAME, { detail: { scope } }));
}

export function subscribeToDesktopRefresh(scopes: DesktopRefreshScope[], listener: () => void): () => void {
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<DesktopRefreshDetail>).detail;
    const scope = detail?.scope ?? "all";

    if (scope === "all" || scopes.includes(scope)) {
      listener();
    }
  };

  window.addEventListener(REFRESH_EVENT_NAME, handler as EventListener);
  return () => {
    window.removeEventListener(REFRESH_EVENT_NAME, handler as EventListener);
  };
}

export function useRefreshSignal(scopes: DesktopRefreshScope[] = ["all"]) {
  const [signal, setSignal] = useState(0);

  useEffect(() => subscribeToDesktopRefresh(scopes, () => setSignal((value) => value + 1)), [scopes]);

  return signal;
}
