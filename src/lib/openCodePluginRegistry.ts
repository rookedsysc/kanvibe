import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

const execFileAsync = promisify(execFile);

interface OpenCodeDebugConfig {
  plugin?: unknown;
}

export async function isOpenCodePluginRegistered(repoPath: string, pluginPath: string): Promise<boolean> {
  try {
    const { stdout } = await execFileAsync("opencode", ["debug", "config"], {
      cwd: repoPath,
      maxBuffer: 1024 * 1024,
    });
    const config = JSON.parse(stdout) as OpenCodeDebugConfig;
    if (!Array.isArray(config.plugin)) {
      return false;
    }

    const expectedPluginUrl = pathToFileURL(pluginPath).href;
    return config.plugin.some((value) => value === expectedPluginUrl);
  } catch {
    return false;
  }
}
