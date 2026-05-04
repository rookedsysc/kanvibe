import os from "os";
import path from "path";

const MAC_LOCAL_COMMAND_PATHS = [
  "/opt/homebrew/bin",
  "/opt/homebrew/sbin",
  "/usr/local/bin",
  "/usr/local/sbin",
  "/opt/local/bin",
  "/opt/local/sbin",
];

function getUserLocalCommandPaths(homeDirectory: string): string[] {
  return [
    path.join(homeDirectory, ".local", "bin"),
    path.join(homeDirectory, ".cargo", "bin"),
    path.join(homeDirectory, ".opencode", "bin"),
    path.join(homeDirectory, "Library", "pnpm"),
    path.join(homeDirectory, ".bun", "bin"),
  ];
}

export function mergeShellPathEntries(pathValues: Array<string | undefined>): string {
  const pathEntries = pathValues
    .flatMap((pathValue) => (pathValue ?? "").split(path.delimiter))
    .map((pathEntry) => pathEntry.trim())
    .filter(Boolean);

  return [...new Set(pathEntries)].join(path.delimiter);
}

export function createLocalShellEnvironment(): Record<string, string> {
  const homeDirectory = process.env.HOME || os.homedir();
  const extraPath = process.platform === "darwin"
    ? [...MAC_LOCAL_COMMAND_PATHS, ...getUserLocalCommandPaths(homeDirectory)].join(path.delimiter)
    : "";

  return {
    ...process.env,
    PATH: mergeShellPathEntries([process.env.PATH, extraPath]),
    LANG: process.env.LANG || "en_US.UTF-8",
    LC_ALL: process.env.LC_ALL || "en_US.UTF-8",
  } as Record<string, string>;
}
