import dgram from "node:dgram";
import { lookup } from "node:dns/promises";
import net from "node:net";
import os from "node:os";
import { parseSSHConfig } from "@/lib/sshConfig";

export const KANVIBE_HOOK_SERVER_PORT = 9736;
export const KANVIBE_DEV_HOOK_SERVER_PORT = 19736;

declare global {
  var __KANVIBE_HOOK_SERVER_PORT__: number | undefined;
}

export function setHookServerPort(port: number): void {
  globalThis.__KANVIBE_HOOK_SERVER_PORT__ = port;
}

export function getHookServerPort(): number {
  return globalThis.__KANVIBE_HOOK_SERVER_PORT__ ?? KANVIBE_HOOK_SERVER_PORT;
}

export function getLocalHookServerUrl(): string {
  return `http://localhost:${getHookServerPort()}`;
}

export async function getRemoteHookServerUrl(sshHost?: string | null): Promise<string> {
  const preferredHost = await getSshRouteIpv4Address(sshHost)
    || getPreferredIpv4Address();

  if (!preferredHost) {
    throw new Error("원격 환경에서 로컬 Hook 서버에 접근할 수 있는 경로를 찾지 못했습니다.");
  }

  return `http://${preferredHost}:${getHookServerPort()}`;
}

export async function getHookServerUrl(sshHost?: string | null): Promise<string> {
  return sshHost ? getRemoteHookServerUrl(sshHost) : getLocalHookServerUrl();
}

async function getSshRouteIpv4Address(sshHost?: string | null): Promise<string | null> {
  if (!sshHost) {
    return null;
  }

  const configs = await parseSSHConfig();
  const config = configs.find((value) => value.host === sshHost);
  const remoteHostname = config?.hostname || sshHost;
  const remotePort = config?.port || 22;
  return getOutboundIpv4Address(remoteHostname, remotePort);
}

async function getOutboundIpv4Address(remoteHostname: string, remotePort: number): Promise<string | null> {
  const remoteAddress = await resolveIpv4Address(remoteHostname);
  if (!remoteAddress) {
    return null;
  }

  return new Promise((resolve) => {
    const socket = dgram.createSocket("udp4");
    let completed = false;

    const finish = (value: string | null) => {
      if (completed) {
        return;
      }

      completed = true;
      socket.close();
      resolve(value);
    };

    socket.once("error", () => finish(null));
    socket.connect(remotePort, remoteAddress, () => {
      const socketAddress = socket.address();
      finish(typeof socketAddress === "object" ? socketAddress.address : null);
    });
  });
}

async function resolveIpv4Address(hostname: string): Promise<string | null> {
  if (net.isIP(hostname) === 4) {
    return hostname;
  }

  try {
    return (await lookup(hostname, { family: 4 })).address;
  } catch {
    return null;
  }
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
