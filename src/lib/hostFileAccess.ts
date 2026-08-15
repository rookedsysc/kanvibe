import { access, mkdir, readFile, readdir, stat, writeFile } from "fs/promises";
import { homedir } from "os";
import path from "path";
import { execGit } from "@/lib/gitOperations";

const remoteHomeDirectoryCache = new Map<string, string>();
const REMOTE_FILE_RECORD_PREFIX = "__KANVIBE_FILE_RECORD__";
// AI history JSONL can exceed the general 10 MiB command cap; 64 MiB keeps reads bounded while covering large sessions.
const REMOTE_TEXT_FILE_MAX_BUFFER_BYTES = 64 * 1024 * 1024;

export interface TextFileReadResult {
  exists: boolean;
  content: string;
}

export function quoteShellArgument(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

export async function getHomeDirectory(sshHost?: string | null): Promise<string> {
  if (!sshHost) {
    return homedir();
  }

  const cached = remoteHomeDirectoryCache.get(sshHost);
  if (cached) {
    return cached;
  }

  const homeDirectory = (await execGit("printf '%s' \"$HOME\"", sshHost)).trim();
  if (!homeDirectory) {
    throw new Error(`${sshHost} 원격 HOME 디렉토리를 확인할 수 없습니다.`);
  }

  remoteHomeDirectoryCache.set(sshHost, homeDirectory);
  return homeDirectory;
}

export async function pathExists(targetPath: string, sshHost?: string | null): Promise<boolean> {
  if (!sshHost) {
    return access(targetPath).then(() => true).catch(() => false);
  }

  const output = await execGit(
    `test -e ${quoteShellArgument(targetPath)} && printf '1' || true`,
    sshHost,
  );
  return output.trim() === "1";
}

export async function readTextFile(targetPath: string, sshHost?: string | null): Promise<string> {
  if (!sshHost) {
    try {
      return await readFile(targetPath, "utf-8");
    } catch {
      return "";
    }
  }

  return execGit(
    `test -f ${quoteShellArgument(targetPath)} && cat ${quoteShellArgument(targetPath)} || true`,
    sshHost,
    { maxBufferBytes: REMOTE_TEXT_FILE_MAX_BUFFER_BYTES },
  );
}

export async function readTextFiles(
  targetPaths: string[],
  sshHost?: string | null,
): Promise<Map<string, TextFileReadResult>> {
  if (targetPaths.length === 0) {
    return new Map();
  }

  if (!sshHost) {
    const entries: Array<[string, TextFileReadResult]> = await Promise.all(targetPaths.map(async (targetPath) => {
      try {
        return [targetPath, { exists: true, content: await readFile(targetPath, "utf-8") }];
      } catch {
        return [targetPath, { exists: false, content: "" }];
      }
    }));

    return new Map<string, TextFileReadResult>(entries);
  }

  const encodedManifest = encodeRemoteTextFileManifest(targetPaths);
  const command = [
    `printf '%s' ${quoteShellArgument(encodedManifest)} | (base64 -d 2>/dev/null || base64 -D) | while IFS= read -r __kanvibe_encoded_file || test -n "$__kanvibe_encoded_file"; do`,
    "test -n \"$__kanvibe_encoded_file\" || continue;",
    "__kanvibe_file=$(printf '%s' \"$__kanvibe_encoded_file\" | (base64 -d 2>/dev/null || base64 -D));",
    `printf '%s\\t%s\\t' ${quoteShellArgument(REMOTE_FILE_RECORD_PREFIX)} "$__kanvibe_file";`,
    "if test -f \"$__kanvibe_file\"; then",
    "printf '1\\t';",
    "(base64 -w 0 \"$__kanvibe_file\" 2>/dev/null || base64 < \"$__kanvibe_file\" | tr -d '\\n');",
    "else",
    "printf '0\\t';",
    "fi;",
    "printf '\\n';",
    "done",
  ].join(" ");
  const output = await execGit(command, sshHost);

  return parseRemoteTextFiles(output, targetPaths);
}

function encodeRemoteTextFileManifest(targetPaths: string[]): string {
  const encodedPaths = targetPaths.map((targetPath) => Buffer.from(targetPath, "utf-8").toString("base64"));
  return Buffer.from(encodedPaths.join("\n"), "utf-8").toString("base64");
}

function parseRemoteTextFiles(
  output: string,
  targetPaths: string[],
): Map<string, TextFileReadResult> {
  const files = new Map<string, TextFileReadResult>(
    targetPaths.map((targetPath) => [targetPath, { exists: false, content: "" }]),
  );

  for (const line of output.split("\n")) {
    if (!line.startsWith(`${REMOTE_FILE_RECORD_PREFIX}\t`)) {
      continue;
    }

    const [, filePath, existsFlag, encodedContent = ""] = line.split("\t");
    if (!filePath) {
      continue;
    }

    files.set(filePath, {
      exists: existsFlag === "1",
      content: existsFlag === "1"
        ? Buffer.from(encodedContent, "base64").toString("utf-8")
        : "",
    });
  }

  return files;
}

export async function writeTextFile(targetPath: string, content: string, sshHost?: string | null): Promise<void> {
  if (!sshHost) {
    await mkdir(path.dirname(targetPath), { recursive: true });
    await writeFile(targetPath, content, "utf-8");
    return;
  }

  const encodedContent = Buffer.from(content, "utf-8").toString("base64");
  await execGit(
    `mkdir -p ${quoteShellArgument(path.posix.dirname(targetPath))} && printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quoteShellArgument(targetPath)}`,
    sshHost,
  );
}

/**
 * 대상 파일이 아직 없을 때만 기록한다.
 * 이미 존재하는 파일은 다른 주체가 확정한 값이므로 검사와 기록 사이에 끼어든 쓰기도 덮지 않아야 한다.
 * 그래서 로컬은 `wx` 플래그로, 원격은 존재 검사와 기록을 한 셸 명령으로 묶어 원자성을 확보한다.
 */
export async function writeTextFileIfAbsent(
  targetPath: string,
  content: string,
  sshHost?: string | null,
): Promise<void> {
  if (!sshHost) {
    await mkdir(path.dirname(targetPath), { recursive: true });
    try {
      await writeFile(targetPath, content, { encoding: "utf-8", flag: "wx" });
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
        throw error;
      }
    }
    return;
  }

  const encodedContent = Buffer.from(content, "utf-8").toString("base64");
  const quotedTargetPath = quoteShellArgument(targetPath);
  await execGit(
    `mkdir -p ${quoteShellArgument(path.posix.dirname(targetPath))} && if [ -e ${quotedTargetPath} ]; then :; else printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quotedTargetPath}; fi`,
    sshHost,
  );
}

export async function readDirectoryFilesBySuffix(
  directoryPath: string,
  suffix: string,
  sshHost?: string | null,
): Promise<string[]> {
  if (!sshHost) {
    try {
      const entries = await readdir(directoryPath, { withFileTypes: true });
      return entries
        .filter((entry) => entry.isFile() && entry.name.endsWith(suffix))
        .map((entry) => path.join(directoryPath, entry.name));
    } catch {
      return [];
    }
  }

  const output = await execGit(
    `test -d ${quoteShellArgument(directoryPath)} && find ${quoteShellArgument(directoryPath)} -maxdepth 1 -type f -name ${quoteShellArgument(`*${suffix}`)} | sort || true`,
    sshHost,
  );

  return output.split("\n").map((value) => value.trim()).filter(Boolean);
}

export async function listFilesRecursivelyBySuffix(
  rootPath: string,
  suffix: string,
  sshHost?: string | null,
): Promise<string[]> {
  if (!sshHost) {
    const entries = await readdir(rootPath, { withFileTypes: true }).catch(() => []);
    const files = await Promise.all(entries.map(async (entry) => {
      const entryPath = path.join(rootPath, entry.name);
      if (entry.isDirectory()) {
        return listFilesRecursivelyBySuffix(entryPath, suffix, sshHost);
      }

      return entry.name.endsWith(suffix) ? [entryPath] : [];
    }));

    return files.flat();
  }

  const output = await execGit(
    `test -d ${quoteShellArgument(rootPath)} && find ${quoteShellArgument(rootPath)} -type f -name ${quoteShellArgument(`*${suffix}`)} | sort || true`,
    sshHost,
  );

  return output.split("\n").map((value) => value.trim()).filter(Boolean);
}

/**
 * 최근에 수정된 파일만 골라 절대 경로와 수정 시각을 함께 돌려준다.
 * 파일 하나씩 mtime을 물어보면 원격에서는 파일 수만큼 SSH 왕복이 생기므로,
 * 원격 경로는 `find`가 필터링과 시각 출력을 한 번에 끝내게 한다.
 */
export async function listFilesModifiedWithin(
  rootPath: string,
  suffix: string,
  withinMs: number,
  sshHost?: string | null,
): Promise<{ filePath: string; mtimeMs: number }[]> {
  const oldestAllowedMs = Date.now() - withinMs;

  if (!sshHost) {
    const filePaths = await listFilesRecursivelyBySuffix(rootPath, suffix, sshHost);
    const records = await Promise.all(filePaths.map(async (filePath) => {
      const mtimeMs = await getFileMtimeMs(filePath, sshHost);
      return mtimeMs === null ? null : { filePath, mtimeMs };
    }));

    return records.filter((record): record is { filePath: string; mtimeMs: number } =>
      record !== null && record.mtimeMs >= oldestAllowedMs);
  }

  const withinMinutes = Math.max(1, Math.ceil(withinMs / 60_000));
  const output = await execGit(
    `test -d ${quoteShellArgument(rootPath)} && find ${quoteShellArgument(rootPath)} -type f -name ${quoteShellArgument(`*${suffix}`)} -mmin -${withinMinutes} -exec sh -c 'for f; do printf "%s\\t%s\\n" "$(stat -c %Y "$f" 2>/dev/null || stat -f %m "$f" 2>/dev/null)" "$f"; done' sh {} + || true`,
    sshHost,
  );

  return output
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const separatorIndex = line.indexOf("\t");
      const secondsValue = Number.parseInt(line.slice(0, separatorIndex), 10);
      const filePath = line.slice(separatorIndex + 1);
      return Number.isNaN(secondsValue) || !filePath ? null : { filePath, mtimeMs: secondsValue * 1000 };
    })
    .filter((record): record is { filePath: string; mtimeMs: number } =>
      record !== null && record.mtimeMs >= oldestAllowedMs);
}

export async function getFileMtimeMs(filePath: string, sshHost?: string | null): Promise<number | null> {
  if (!sshHost) {
    try {
      return (await stat(filePath)).mtimeMs;
    } catch {
      return null;
    }
  }

  const output = await execGit(
    `test -e ${quoteShellArgument(filePath)} && (stat -c %Y ${quoteShellArgument(filePath)} 2>/dev/null || stat -f %m ${quoteShellArgument(filePath)} 2>/dev/null) || true`,
    sshHost,
  );
  const timestamp = Number.parseInt(output.trim(), 10);
  return Number.isNaN(timestamp) ? null : timestamp * 1000;
}
