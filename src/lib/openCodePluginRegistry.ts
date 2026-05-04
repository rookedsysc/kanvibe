import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";

const execFileAsync = promisify(execFile);

interface OpenCodeDebugConfig {
  plugin?: unknown;
}

function normalizePluginEntries(values: unknown): string[] {
  if (!Array.isArray(values)) {
    return [];
  }

  return values.filter((value): value is string => typeof value === "string");
}

export function extractRegisteredPluginUrls(output: string): string[] {
  try {
    const config = JSON.parse(output) as OpenCodeDebugConfig;
    const pluginEntries = normalizePluginEntries(config.plugin);
    if (pluginEntries.length > 0) {
      return pluginEntries;
    }
  } catch {
    // OpenCode debug output can embed invalid JSON in later sections. Fall back to the top-level plugin block.
  }

  const pluginBlockMatch = output.match(/"plugin"\s*:\s*\[([\s\S]*?)\]\s*,\s*"[^"]+"\s*:/);
  const pluginBlock = pluginBlockMatch?.[1];
  if (!pluginBlock) {
    return [];
  }

  return Array.from(pluginBlock.matchAll(/"((?:\\.|[^"\\])*)"/g), ([, value]) => JSON.parse(`"${value}"`) as string);
}

export function isKanvibePluginUrl(value: string): boolean {
  return /\/kanvibe-plugin\.(?:[cm]?js|ts)$/i.test(value);
}

export function extractRegisteredKanvibePluginUrls(output: string): string[] {
  return extractRegisteredPluginUrls(output).filter(isKanvibePluginUrl);
}

export async function getOpenCodeRegisteredKanvibePluginUrls(repoPath: string): Promise<string[]> {
  try {
    const { stdout } = await execFileAsync("opencode", ["debug", "config"], {
      cwd: repoPath,
      env: createLocalShellEnvironment(),
      maxBuffer: 1024 * 1024,
    });

    return extractRegisteredKanvibePluginUrls(stdout);
  } catch {
    return [];
  }
}

export async function isOpenCodePluginRegistered(repoPath: string, pluginPath: string): Promise<boolean> {
  try {
    const expectedPluginUrl = pathToFileURL(pluginPath).href;
    return (await getOpenCodeRegisteredKanvibePluginUrls(repoPath)).some((value) => value === expectedPluginUrl);
  } catch {
    return false;
  }
}
