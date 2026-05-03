export const REQUEST_ACTIVE_TERMINAL_FOCUS_EVENT = "kanvibe:request-terminal-focus";
export const TERMINAL_FOCUS_BLOCKER_ATTRIBUTE = "data-terminal-focus-blocker";

export function hasTerminalFocusBlocker() {
  if (typeof document === "undefined") {
    return false;
  }

  return document.querySelector(`[${TERMINAL_FOCUS_BLOCKER_ATTRIBUTE}="true"]`) !== null;
}

export function requestActiveTerminalFocus() {
  if (typeof window === "undefined") {
    return;
  }

  window.dispatchEvent(new Event(REQUEST_ACTIVE_TERMINAL_FOCUS_EVENT));
}

export function requestActiveTerminalFocusAfterUiSettles() {
  if (typeof window === "undefined") {
    return;
  }

  const dispatchFocusRequest = () => {
    requestActiveTerminalFocus();
  };

  if (typeof window.requestAnimationFrame === "function") {
    window.requestAnimationFrame(dispatchFocusRequest);
    return;
  }

  window.setTimeout(dispatchFocusRequest, 0);
}
