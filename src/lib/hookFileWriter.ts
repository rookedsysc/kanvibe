import { chmod, mkdir, writeFile } from "fs/promises";
import path from "path";
import { execGit } from "@/lib/gitOperations";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import type { ShellHookProviderFile } from "@/lib/shellHookProvider";

/**
 * hook 설치 산출물(스크립트·설정 파일)을 로컬 또는 원격 repo에 기록한다.
 * 로컬/원격 설치가 같은 파일 목록을 공유하도록 쓰기 경로만 분리한다.
 */
export async function writeHookProviderFiles(
  files: ShellHookProviderFile[],
  sshHost?: string | null,
): Promise<void> {
  if (files.length === 0) {
    return;
  }

  if (!sshHost) {
    await writeLocalHookProviderFiles(files);
    return;
  }

  await writeRemoteHookProviderFiles(files, sshHost);
}

async function writeLocalHookProviderFiles(files: ShellHookProviderFile[]): Promise<void> {
  for (const { filePath, content, mode } of files) {
    await mkdir(path.dirname(filePath), { recursive: true });
    await writeFile(filePath, content, "utf-8");

    if (mode) {
      await chmod(filePath, mode);
    }
  }
}

async function writeRemoteHookProviderFiles(
  files: ShellHookProviderFile[],
  sshHost: string,
): Promise<void> {
  const command = files.map(({ filePath, content, mode }) => {
    const encodedContent = Buffer.from(content, "utf-8").toString("base64");
    const parts = [
      `mkdir -p ${quoteShellArgument(path.posix.dirname(filePath))}`,
      `printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quoteShellArgument(filePath)}`,
    ];

    if (mode) {
      parts.push(`chmod ${mode.toString(8)} ${quoteShellArgument(filePath)}`);
    }

    return parts.join(" && ");
  }).join(" && ");

  await execGit(command, sshHost);
}
