export const INITIAL_DESKTOP_LOAD_TIMEOUT_MS = 5000;

export function logDesktopInitialLoadTimeout(
  routeName: string,
  details: Record<string, unknown> = {},
) {
  window.kanvibeDesktop?.logRendererError?.("renderer:initial-load-timeout", {
    routeName,
    timeoutMs: INITIAL_DESKTOP_LOAD_TIMEOUT_MS,
    ...details,
  });
}
