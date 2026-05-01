import { clearDesktopSession, createDesktopSession, hasDesktopSession } from "@/desktop/main/sessionStore";

function getConfiguredUsername(): string {
  return process.env.KANVIBE_USER || "admin";
}

function getConfiguredPassword(): string {
  return process.env.KANVIBE_PASSWORD || "changeme";
}

export function validateCredentials(username: string, password: string): boolean {
  return username === getConfiguredUsername() && password === getConfiguredPassword();
}

export async function login(username: string, password: string): Promise<{ success: boolean; error?: string }> {
  if (!username || !password) {
    return { success: false, error: "아이디와 비밀번호를 입력해주세요." };
  }

  if (!validateCredentials(username, password)) {
    return { success: false, error: "아이디 또는 비밀번호가 올바르지 않습니다." };
  }

  createDesktopSession();
  return { success: true };
}

export async function logout(): Promise<void> {
  clearDesktopSession();
}

export async function getSessionState(): Promise<{ isAuthenticated: boolean }> {
  return { isAuthenticated: hasDesktopSession() };
}
