import { chmod, mkdir, writeFile } from "fs/promises";
import path from "path";
import { execGit } from "@/lib/gitOperations";
import { getHookServerUrl } from "@/lib/hookEndpoint";
import {
  buildHookInstallBootstrapCommand,
  HOOK_INSTALL_SUCCESS_MARKER,
  issueHookInstallScript,
} from "@/lib/hookInstallBundle";
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

/**
 * 원격 설치는 Hook 서버에서 설치 스크립트를 내려받아 실행하는 경로를 먼저 쓴다.
 * 파일 본문을 SSH 명령줄에 싣지 않아 명령이 짧아지고, 인용 처리에 걸릴 여지가 사라진다.
 * 원격에서 Hook 서버로 오는 경로가 막혀 있으면 기존 SSH 주입 방식으로 물러난다.
 */
async function writeRemoteHookProviderFiles(
  files: ShellHookProviderFile[],
  sshHost: string,
): Promise<void> {
  try {
    await installRemoteHookProviderFilesOverHttp(files, sshHost);
    return;
  } catch (error) {
    console.warn("[hooks] HTTP 설치 경로 실패, SSH 주입으로 대체합니다", {
      sshHost,
      error: error instanceof Error ? error.message : String(error),
    });
  }

  await injectRemoteHookProviderFilesOverSsh(files, sshHost);
}

async function installRemoteHookProviderFilesOverHttp(
  files: ShellHookProviderFile[],
  sshHost: string,
): Promise<void> {
  const hookServerUrl = await getHookServerUrl(sshHost);
  const installToken = issueHookInstallScript(files);
  const output = await execGit(
    buildHookInstallBootstrapCommand(hookServerUrl, installToken),
    sshHost,
  );

  if (!output.includes(HOOK_INSTALL_SUCCESS_MARKER)) {
    throw new Error("원격 hook 설치 스크립트가 완료 표시를 남기지 않았습니다.");
  }
}

async function injectRemoteHookProviderFilesOverSsh(
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
