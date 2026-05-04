import { mkdir, readFile } from "fs/promises";
import { homedir } from "os";
import path from "path";

export interface SSHHostConfig {
  host: string;
  hostname: string;
  port: number;
  username: string;
  privateKeyPath: string;
}

export interface SSHConnectionReuseOptions {
  controlPath: string;
  controlPersist: string;
}

export interface SSHConnectionHealthOptions {
  connectTimeoutSeconds: number;
  serverAliveIntervalSeconds: number;
  serverAliveCountMax: number;
}

const SSH_CONFIG_CACHE_TTL_MS = 5000;
const KANVIBE_SSH_CONTROL_PERSIST = "10m";
const KANVIBE_SSH_CONNECT_TIMEOUT_SECONDS = 8;
const KANVIBE_SSH_SERVER_ALIVE_INTERVAL_SECONDS = 5;
const KANVIBE_SSH_SERVER_ALIVE_COUNT_MAX = 2;
let cachedHosts: SSHHostConfig[] | null = null;
let cacheExpiresAt = 0;
let inFlightParse: Promise<SSHHostConfig[]> | null = null;

export function getSSHDestination(config: Pick<SSHHostConfig, "host" | "hostname" | "username">): string {
  if (config.host) {
    return config.host;
  }

  return `${config.username}@${config.hostname}`;
}

export function buildSSHArgs(
  config: Pick<SSHHostConfig, "host" | "hostname" | "port" | "username" | "privateKeyPath">,
  options?: {
    forceTty?: boolean;
    disableTty?: boolean;
    trustedX11Forwarding?: boolean;
    connectionReuse?: SSHConnectionReuseOptions;
    connectionHealth?: SSHConnectionHealthOptions;
  },
): string[] {
  const args = [
    "-i",
    config.privateKeyPath,
    "-p",
    String(config.port),
    "-o",
    "BatchMode=yes",
    "-o",
    "IdentitiesOnly=yes",
  ];

  if (options?.trustedX11Forwarding) {
    args.push("-Y");
  }

  if (options?.forceTty) {
    args.push("-tt");
  } else if (options?.disableTty) {
    args.push("-T");
  }

  if (options?.connectionReuse) {
    args.push(
      "-o",
      "ControlMaster=auto",
      "-o",
      `ControlPersist=${options.connectionReuse.controlPersist}`,
      "-o",
      `ControlPath=${options.connectionReuse.controlPath}`,
    );
  }

  if (options?.connectionHealth) {
    args.push(
      "-o",
      `ConnectTimeout=${options.connectionHealth.connectTimeoutSeconds}`,
      "-o",
      `ServerAliveInterval=${options.connectionHealth.serverAliveIntervalSeconds}`,
      "-o",
      `ServerAliveCountMax=${options.connectionHealth.serverAliveCountMax}`,
    );
  }

  args.push(getSSHDestination(config));
  return args;
}

export function getKanvibeSSHControlDirectory(): string {
  return path.join(homedir(), ".kanvibe");
}

export async function ensureKanvibeSSHControlDirectory(): Promise<void> {
  await mkdir(getKanvibeSSHControlDirectory(), { recursive: true, mode: 0o700 });
}

export function getKanvibeSSHConnectionReuseOptions(): SSHConnectionReuseOptions {
  return {
    controlPath: path.join(getKanvibeSSHControlDirectory(), "ssh-%C"),
    controlPersist: KANVIBE_SSH_CONTROL_PERSIST,
  };
}

export function getKanvibeSSHConnectionHealthOptions(): SSHConnectionHealthOptions {
  return {
    connectTimeoutSeconds: KANVIBE_SSH_CONNECT_TIMEOUT_SECONDS,
    serverAliveIntervalSeconds: KANVIBE_SSH_SERVER_ALIVE_INTERVAL_SECONDS,
    serverAliveCountMax: KANVIBE_SSH_SERVER_ALIVE_COUNT_MAX,
  };
}

export function hasLocalX11Display(env: NodeJS.ProcessEnv = process.env): boolean {
  return Boolean(env.DISPLAY);
}

/**
 * ~/.ssh/config 파일을 파싱하여 호스트 목록을 반환한다.
 * Host, HostName, User, IdentityFile, Port 필드를 추출한다.
 */
export async function parseSSHConfig(): Promise<SSHHostConfig[]> {
  const now = Date.now();
  if (cachedHosts && cacheExpiresAt > now) {
    return cachedHosts;
  }

  if (inFlightParse) {
    return inFlightParse;
  }

  const configPath = path.join(homedir(), ".ssh", "config");

  inFlightParse = (async () => {
    let content: string;
    try {
      content = await readFile(configPath, "utf-8");
    } catch {
      cachedHosts = [];
      cacheExpiresAt = Date.now() + SSH_CONFIG_CACHE_TTL_MS;
      return [];
    }

    const hosts: SSHHostConfig[] = [];
    let current: (Partial<SSHHostConfig> & { aliases?: string[] }) | null = null;

    for (const rawLine of content.split("\n")) {
      const line = rawLine.trim();
      if (!line || line.startsWith("#")) continue;

      const [key, ...valueParts] = line.split(/\s+/);
      const value = valueParts.join(" ");

      if (key.toLowerCase() === "host") {
        if (current?.aliases?.length && current.hostname) {
          hosts.push(...expandHostAliases(current));
        }
        current = { aliases: valueParts };
      } else if (current) {
        switch (key.toLowerCase()) {
          case "hostname":
            current.hostname = value;
            break;
          case "user":
            current.username = value;
            break;
          case "port":
            current.port = parseInt(value, 10);
            break;
          case "identityfile":
            current.privateKeyPath = value.replace("~", homedir());
            break;
        }
      }
    }

    if (current?.aliases?.length && current.hostname) {
      hosts.push(...expandHostAliases(current));
    }

    cachedHosts = hosts;
    cacheExpiresAt = Date.now() + SSH_CONFIG_CACHE_TTL_MS;
    return hosts;
  })();

  try {
    return await inFlightParse;
  } finally {
    inFlightParse = null;
  }
}

function fillDefaults(partial: Partial<SSHHostConfig>): SSHHostConfig {
  return {
    host: partial.host!,
    hostname: partial.hostname!,
    port: partial.port || 22,
    username: partial.username || "root",
    privateKeyPath: partial.privateKeyPath || path.join(homedir(), ".ssh", "id_rsa"),
  };
}

function expandHostAliases(
  partial: Partial<SSHHostConfig> & { aliases?: string[] },
): SSHHostConfig[] {
  return (partial.aliases || [])
    .filter((alias) => alias && !/[*!?]/.test(alias))
    .map((alias) => fillDefaults({ ...partial, host: alias }));
}

/** 사용 가능한 SSH 호스트 이름 목록을 반환한다 */
export async function getAvailableHosts(): Promise<string[]> {
  const configs = await parseSSHConfig();
  return configs.map((c) => c.host);
}
