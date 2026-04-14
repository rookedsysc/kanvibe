import os from "node:os";

export const KANVIBE_HOOK_SERVER_PORT = 9736;

export function getHookServerToken(): string {
  return process.env.KANVIBE_HOOK_TOKEN || "";
}

export function getLocalHookServerUrl(): string {
  return `http://localhost:${KANVIBE_HOOK_SERVER_PORT}`;
}

export function getRemoteHookServerUrl(): string {
  const preferredHost = process.env.KANVIBE_EXTERNAL_HOST || getPreferredIpv4Address();

  if (!preferredHost) {
    throw new Error("로컬 Hook 서버에 접근할 수 있는 IP를 찾지 못했습니다. KANVIBE_EXTERNAL_HOST를 설정해 주세요.");
  }

  return `http://${preferredHost}:${KANVIBE_HOOK_SERVER_PORT}`;
}

export function getHookServerUrl(sshHost?: string | null): string {
  return sshHost ? getRemoteHookServerUrl() : getLocalHookServerUrl();
}

function getPreferredIpv4Address(): string | null {
  const interfaces = os.networkInterfaces();

  for (const addresses of Object.values(interfaces)) {
    for (const address of addresses || []) {
      if (address.family !== "IPv4" || address.internal) {
        continue;
      }

      if (
        address.address.startsWith("10.") ||
        address.address.startsWith("192.168.") ||
        /^172\.(1[6-9]|2\d|3[0-1])\./.test(address.address)
      ) {
        return address.address;
      }
    }
  }

  for (const addresses of Object.values(interfaces)) {
    for (const address of addresses || []) {
      if (address.family === "IPv4" && !address.internal) {
        return address.address;
      }
    }
  }

  return null;
}
