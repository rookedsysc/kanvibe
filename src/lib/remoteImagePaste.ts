import { randomUUID } from "crypto";
import { spawn } from "child_process";
import { mkdtemp, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { buildSCPArgs, type SSHHostConfig } from "@/lib/sshConfig";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";

const REMOTE_TEMP_DIRECTORY = "/tmp";

/** `data:image/png;base64,<payload>` 형태의 문자열에서 실제 바이트를 꺼낸다 */
export function decodeDataUrlToBuffer(dataUrl: string): Buffer {
  const commaIndex = dataUrl.indexOf(",");
  if (commaIndex === -1) {
    throw new Error("잘못된 이미지 데이터입니다.");
  }

  return Buffer.from(dataUrl.slice(commaIndex + 1), "base64");
}

/**
 * 붙여넣은 이미지가 원격에 저장될 경로.
 * SSH로 접속 가능한 원격 호스트는 사실상 항상 유닉스 계열이라 `/tmp`를 고정으로 가정한다.
 */
export function buildRemoteImagePastePath(uuid: string): string {
  return path.posix.join(REMOTE_TEMP_DIRECTORY, `kanvibe-paste-${uuid}.png`);
}

/** 클립보드 이미지를 scp로 원격 호스트에 올리고, 성공하면 원격 경로를 반환한다 */
export async function transferImageToRemoteHost(
  sshConfig: SSHHostConfig,
  imageBuffer: Buffer,
): Promise<string> {
  return transferFileToRemoteHost(sshConfig, imageBuffer, "png");
}

export async function transferFileToRemoteHost(
  sshConfig: SSHHostConfig,
  buffer: Buffer,
  extension: string,
): Promise<string> {
  if (!/^[a-z0-9]{1,12}$/.test(extension)) throw new Error("첨부 파일 확장자가 올바르지 않습니다.");
  const uuid = randomUUID();
  const remotePath = extension === "png" ? buildRemoteImagePastePath(uuid) : path.posix.join(REMOTE_TEMP_DIRECTORY, `kanvibe-paste-${uuid}.${extension}`);
  const localDirectory = await mkdtemp(path.join(tmpdir(), "kanvibe-paste-"));
  const localPath = path.join(localDirectory, `${uuid}.${extension}`);

  try {
    await writeFile(localPath, buffer, { mode: 0o600 });
    await runSCP(buildSCPArgs(sshConfig, localPath, remotePath));
    return remotePath;
  } finally {
    await rm(localDirectory, { recursive: true, force: true });
  }
}

function runSCP(args: string[]): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn("scp", args, { env: createLocalShellEnvironment() });
    let stderr = "";

    child.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });

    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`scp 전송 실패 (exit ${code}): ${stderr.trim()}`));
      }
    });
  });
}
