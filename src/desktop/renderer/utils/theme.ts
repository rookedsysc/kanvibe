import type { ThemePreference } from "@/desktop/renderer/actions/appSettings";

export const THEME_PREFERENCE_CHANGED_EVENT = "kanvibe:theme-preference-changed";

export function resolveThemePreference(themePreference: ThemePreference): "light" | "dark" {
  if (themePreference !== "system") {
    return themePreference;
  }

  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

export function applyThemePreference(themePreference: ThemePreference) {
  const resolvedTheme = resolveThemePreference(themePreference);
  document.documentElement.dataset.themePreference = themePreference;
  document.documentElement.dataset.theme = resolvedTheme;
  document.documentElement.style.colorScheme = resolvedTheme;
}

export function notifyThemePreferenceChanged(themePreference: ThemePreference) {
  window.dispatchEvent(new CustomEvent<ThemePreference>(THEME_PREFERENCE_CHANGED_EVENT, {
    detail: themePreference,
  }));
}
