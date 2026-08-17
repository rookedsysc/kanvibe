import { rename, unlink, writeFile } from "fs/promises";
import path from "path";

/** 자격증명은 소유자만 읽을 수 있어야 하고, CLI가 쓰는 권한과 같아야 한다 */
const CREDENTIALS_FILE_MODE = 0o600;

/**
 * 자격증명 파일을 같은 디렉터리의 임시 파일에 먼저 쓰고 rename으로 갈아 끼운다.
 *
 * refresh 토큰은 한 번 쓰고 회전되므로, 갱신된 값을 반쯤 기록한 파일이 남으면
 * 사용자의 CLI 로그인이 통째로 끊긴다. rename은 같은 파일시스템 안에서 원자적이라
 * 읽는 쪽은 옛 파일과 새 파일 중 하나만 보게 된다.
 */
export async function writeCredentialsAtomically(
  targetPath: string,
  contents: string,
): Promise<void> {
  const temporaryPath = path.join(
    path.dirname(targetPath),
    `.${path.basename(targetPath)}.kanvibe-${process.pid}.tmp`,
  );

  await writeFile(temporaryPath, contents, { encoding: "utf-8", mode: CREDENTIALS_FILE_MODE });

  try {
    await rename(temporaryPath, targetPath);
  } catch (error) {
    await unlink(temporaryPath).catch(() => {});
    throw error;
  }
}
