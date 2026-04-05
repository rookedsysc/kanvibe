import { useEffect, useState } from "react";

const REFRESH_EVENT_NAME = "kanvibe:navigation-refresh";

export function triggerDesktopRefresh() {
  window.dispatchEvent(new Event(REFRESH_EVENT_NAME));
}

export function subscribeToDesktopRefresh(listener: () => void): () => void {
  window.addEventListener(REFRESH_EVENT_NAME, listener);
  return () => {
    window.removeEventListener(REFRESH_EVENT_NAME, listener);
  };
}

export function useRefreshSignal() {
  const [signal, setSignal] = useState(0);

  useEffect(() => subscribeToDesktopRefresh(() => setSignal((value) => value + 1)), []);

  return signal;
}
