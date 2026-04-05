import { invokeDesktop } from "@/desktop/renderer/ipc";
import { getSafeLocale } from "@/desktop/renderer/utils/locales";

function getCurrentLocale(): string {
  const path = window.location.hash.replace(/^#/, "") || "/ko/login";
  return getSafeLocale(path.split("/").filter(Boolean)[0]);
}

function navigateTo(href: string) {
  window.location.hash = `#${href}`;
}

export async function loginAction(formData: FormData): Promise<{ error: string } | void> {
  const username = String(formData.get("username") || "");
  const password = String(formData.get("password") || "");
  const result = await invokeDesktop<{ success: boolean; error?: string }>("auth", "login", username, password);

  if (!result.success) {
    return { error: result.error || "로그인에 실패했습니다." };
  }

  const locale = getCurrentLocale();
  window.dispatchEvent(new Event("kanvibe:session-changed"));
  navigateTo(`/${locale}`);
}

export async function logoutAction(): Promise<void> {
  await invokeDesktop<void>("auth", "logout");
  const locale = getCurrentLocale();
  window.dispatchEvent(new Event("kanvibe:session-changed"));
  navigateTo(`/${locale}/login`);
}

export async function getSessionState(): Promise<{ isAuthenticated: boolean }> {
  return invokeDesktop("auth", "getSessionState");
}
