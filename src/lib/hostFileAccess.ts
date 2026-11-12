import { access, chmod, lstat, mkdir, readFile, readdir, rename, rmdir, stat, unlink, writeFile } from "fs/promises";
import { randomUUID } from "crypto";
import { homedir, hostname } from "os";
import path from "path";
import { execGit, resolvePathForShell, type ExecGitOptions } from "@/lib/gitOperations";

const remoteHomeDirectoryCache = new Map<string, string>();
const REMOTE_FILE_RECORD_PREFIX = "__KANVIBE_FILE_RECORD__";
const REMOTE_FILE_RECORD_END = "__KANVIBE_FILE_RECORD_END__";
// AI history JSONL can exceed the general 10 MiB command cap; 64 MiB keeps reads bounded while covering large sessions.
const REMOTE_TEXT_FILE_MAX_BUFFER_BYTES = 64 * 1024 * 1024;
const FILE_UPDATE_LOCK_ATTEMPTS = 200;
const FILE_UPDATE_LOCK_RETRY_MS = 50;
const FILE_UPDATE_OWNER_INITIALIZATION_ATTEMPTS = 10;
const REMOTE_FILE_UPDATE_READY = "__KANVIBE_FILE_UPDATE_READY__";
const REMOTE_FILE_UPDATE_DONE = "__KANVIBE_FILE_UPDATED__";
const REMOTE_FILE_UPDATE_BUSY = "__KANVIBE_FILE_UPDATE_BUSY__";
const REMOTE_FILE_UPDATE_UNOWNED = "__KANVIBE_FILE_UPDATE_UNOWNED__";

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

export async function isPathMissing(targetPath: string, sshHost?: string | null): Promise<boolean> {
  if (!sshHost) {
    try {
      await lstat(targetPath);
      return false;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") return true;
      throw error;
    }
  }

  const output = await execGit([
    `__kanvibe_probe=${resolvePathForShell(targetPath, sshHost)}`,
    'if test -e "$__kanvibe_probe" || test -L "$__kanvibe_probe"; then',
    "  printf 'present'",
    "else",
    '  while ! test -e "$__kanvibe_probe" && ! test -L "$__kanvibe_probe"; do',
    '    __kanvibe_parent=$(dirname "$__kanvibe_probe")',
    '    test "$__kanvibe_parent" != "$__kanvibe_probe" || exit 1',
    '    __kanvibe_probe=$__kanvibe_parent',
    '  done',
    '  test -d "$__kanvibe_probe" && test -r "$__kanvibe_probe" && test -x "$__kanvibe_probe" || exit 1',
    "  printf 'missing'",
    "fi",
  ].join("\n"), sshHost);
  if (output.trim() === "missing") return true;
  if (output.trim() === "present") return false;
  throw new Error(`${targetPath} 경로를 확인하지 못했습니다.`);
}

/** 경로 가운데 심볼릭 링크인 첫 경로를 돌려준다. 없는 경로는 링크가 아니다 */
export async function findSymbolicLinkPath(targetPaths: string[], sshHost?: string | null, options?: Pick<ExecGitOptions, "strictHostKeyChecking">): Promise<string | null> {
  if (!sshHost) {
    for (const targetPath of targetPaths) {
      const isSymbolicLink = await lstat(targetPath).then((entry) => entry.isSymbolicLink()).catch(() => false);
      if (isSymbolicLink) {
        return targetPath;
      }
    }
    return null;
  }

  const quotedPaths = targetPaths.map(quoteShellArgument).join(" ");
  const output = await execGit(
    `for __kanvibe_path in ${quotedPaths}; do if test -L "$__kanvibe_path"; then printf '%s' "$__kanvibe_path"; break; fi; done`,
    sshHost,
    ...(options ? [options] as const : [] as const),
  );
  return output.trim() || null;
}

export async function readTextFile(targetPath: string, sshHost?: string | null, options?: Pick<ExecGitOptions, "strictHostKeyChecking">): Promise<string> {
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
    { maxBufferBytes: REMOTE_TEXT_FILE_MAX_BUFFER_BYTES, ...options },
  );
}

export async function readTextFiles(
  targetPaths: string[],
  sshHost?: string | null,
  options?: Pick<ExecGitOptions, "strictHostKeyChecking">,
): Promise<Map<string, TextFileReadResult>> {
  if (targetPaths.length === 0) {
    return new Map();
  }

  if (!sshHost) {
    const entries: Array<[string, TextFileReadResult]> = await Promise.all(targetPaths.map(async (targetPath) => {
      try {
        return [targetPath, { exists: true, content: await readFile(targetPath, "utf-8") }];
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
        return [targetPath, { exists: false, content: "" }];
      }
    }));

    return new Map<string, TextFileReadResult>(entries);
  }

  const encodedManifest = encodeRemoteTextFileManifest([...new Set(targetPaths)]);
  const command = [
    `__kanvibe_manifest=$(printf '%s' ${quoteShellArgument(encodedManifest)} | (base64 -d 2>/dev/null || base64 -D)) || exit $?;`,
    'printf "%s\\n" "$__kanvibe_manifest" | while IFS= read -r __kanvibe_encoded_file || test -n "$__kanvibe_encoded_file"; do',
    'test -n "$__kanvibe_encoded_file" || continue;',
    '__kanvibe_file=$(printf "%s" "$__kanvibe_encoded_file" | (base64 -d 2>/dev/null || base64 -D)) || exit $?;',
    // 접근 불가한 부모 아래의 파일은 test -e도 false이므로, 부재로 기록하기 전에 탐색 권한을 확인한다.
    '__kanvibe_parent=$(dirname "$__kanvibe_file") || exit $?;',
    'while ! test -e "$__kanvibe_parent" && ! test -L "$__kanvibe_parent"; do',
    '__kanvibe_next_parent=$(dirname "$__kanvibe_parent") || exit $?;',
    '[ "$__kanvibe_parent" != "$__kanvibe_next_parent" ] || exit 1;',
    '__kanvibe_parent=$__kanvibe_next_parent; done;',
    'test -d "$__kanvibe_parent" && test -x "$__kanvibe_parent" || exit 1;',
    'if test -f "$__kanvibe_file"; then',
    // 파이프의 마지막 tr 성공이 base64 실패를 가리지 않도록 인코딩 결과부터 확인한다.
    '__kanvibe_content=$(base64 -w 0 "$__kanvibe_file" 2>/dev/null || base64 < "$__kanvibe_file") || exit $?;',
    `printf '%s\\t%s\\t1\\t' ${quoteShellArgument(REMOTE_FILE_RECORD_PREFIX)} "$__kanvibe_file";`,
    'printf "%s" "$__kanvibe_content" | tr -d "\\n";',
    'elif test -e "$__kanvibe_file" || test -L "$__kanvibe_file"; then exit 1;',
    `else printf '%s\\t%s\\t0\\t' ${quoteShellArgument(REMOTE_FILE_RECORD_PREFIX)} "$__kanvibe_file"; fi;`,
    // execGit의 trim()이 빈 내용 필드의 탭까지 지우지 않도록 종료 표식을 붙인다.
    `printf '\\t%s\\n' ${quoteShellArgument(REMOTE_FILE_RECORD_END)};`,
    "done",
  ].join(" ");
  const output = await execGit(command, sshHost, ...(options ? [options] as const : [] as const));

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
  const files = new Map<string, TextFileReadResult>();
  const expectedPaths = new Set(targetPaths);
  for (const line of output.split("\n")) {
    if (!line.startsWith(`${REMOTE_FILE_RECORD_PREFIX}\t`)) continue;
    const fields = line.split("\t");
    const [, filePath, existsFlag, encodedContent, recordEnd] = fields;
    if (fields.length !== 5 || recordEnd !== REMOTE_FILE_RECORD_END || !expectedPaths.has(filePath) || files.has(filePath)
      || (existsFlag !== "0" && existsFlag !== "1")
      || (existsFlag === "0" && encodedContent !== "")
      || Buffer.from(encodedContent, "base64").toString("base64") !== encodedContent) {
      throw new Error("원격 파일 검사 결과가 손상되었습니다.");
    }
    files.set(filePath, {
      exists: existsFlag === "1",
      content: existsFlag === "1" ? Buffer.from(encodedContent, "base64").toString("utf-8") : "",
    });
  }
  if (files.size !== expectedPaths.size) throw new Error("원격 파일 검사 결과가 누락되었습니다.");
  return files;
}

export async function writeTextFile(targetPath: string, content: string, sshHost?: string | null, options?: Pick<ExecGitOptions, "strictHostKeyChecking">): Promise<void> {
  if (!sshHost) {
    await mkdir(path.dirname(targetPath), { recursive: true });
    await writeFile(targetPath, content, "utf-8");
    return;
  }

  const encodedContent = Buffer.from(content, "utf-8").toString("base64");
  await execGit(
    `mkdir -p ${quoteShellArgument(path.posix.dirname(targetPath))} && printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quoteShellArgument(targetPath)}`,
    sshHost,
    ...(options ? [options] as const : [] as const),
  );
}

function buildRemoteUmaskCommand(mode?: number): string {
  return mode !== undefined ? `umask ${(~mode & 0o777).toString(8)} && ` : "";
}

/**
 * 저장소 측 잠금으로 여러 프로세스와 SSH client의 읽기·수정·교체를 직렬화한다.
 * `mode`를 주면 내용이 담기는 임시 파일부터 그 권한으로 만들어, 교체 전후 어느 순간에도 넓은 권한으로 열리지 않는다.
 */
export async function updateTextFile(
  targetPath: string,
  update: (content: string) => string | Promise<string>,
  sshHost?: string | null,
  mode?: number,
  options?: Pick<ExecGitOptions, "strictHostKeyChecking">,
): Promise<void> {
  const lockPath = `${targetPath}.lock`;
  const temporaryPath = `${targetPath}.${randomUUID()}.tmp`;
  const directoryPath = (sshHost ? path.posix : path).dirname(targetPath);
  if (sshHost) {
    await updateRemoteTextFile(targetPath, temporaryPath, update, sshHost, mode, options);
    return;
  }
  await mkdir(directoryPath, { recursive: true });
  const ownerPath = path.join(lockPath, `owner-${process.pid}-${randomUUID()}-${Buffer.from(hostname()).toString("hex")}`);
  let unownedObservations = 0;
  for (let attempt = 0; ; attempt++) {
    const state = await tryAcquireLocalFileLock(lockPath, ownerPath);
    if (state === "acquired") break;
    if (state === "unowned" && ++unownedObservations >= FILE_UPDATE_OWNER_INITIALIZATION_ATTEMPTS) {
      throw new Error(`소유자를 확인할 수 없는 파일 잠금입니다. 사용 중인 작업이 없는지 확인한 뒤 잠금을 백업 이동하세요: ${lockPath}`);
    }
    if (attempt >= FILE_UPDATE_LOCK_ATTEMPTS - 1) throw new Error(`파일 잠금 대기 시간이 초과되었습니다: ${lockPath}`);
    await new Promise((resolve) => setTimeout(resolve, FILE_UPDATE_LOCK_RETRY_MS));
  }

  try {
    const currentContent = await readFile(targetPath, "utf-8").catch((error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error;
      return "";
    });
    const nextContent = await update(currentContent);
    if (!(await lstat(ownerPath)).isDirectory()) throw new Error(`파일 잠금 소유권이 변경되었습니다: ${lockPath}`);
    await writeFile(temporaryPath, nextContent, { encoding: "utf-8", flag: "wx", ...(mode !== undefined ? { mode } : {}) });
    if (mode !== undefined) {
      await chmod(temporaryPath, mode);
    }
    await rename(temporaryPath, targetPath);
  } finally {
    await unlink(temporaryPath).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error;
    });
    await rmdir(ownerPath).then(() => rmdir(lockPath)).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error;
    });
  }
}

async function tryAcquireLocalFileLock(lockPath: string, ownerPath: string): Promise<"acquired" | "busy" | "unowned"> {
  try {
    await mkdir(lockPath, { mode: 0o700 });
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    const entry = await lstat(lockPath).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error;
      return null;
    });
    if (!entry) return "busy";
    if (!entry.isDirectory() || entry.isSymbolicLink()) throw new Error(`파일 잠금 경로가 디렉터리가 아닙니다: ${lockPath}`);
    const owners = await readdir(lockPath).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error;
      return [];
    });
    const owner = owners.length === 1 ? owners[0] : "";
    const match = /^owner-([1-9][0-9]*)-[a-zA-Z0-9-]+-([a-f0-9]+)$/.exec(owner);
    if (!match || match[2] !== Buffer.from(hostname()).toString("hex")) return "unowned";
    try {
      process.kill(Number(match[1]), 0);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ESRCH") {
        await rmdir(path.join(lockPath, owner)).then(() => rmdir(lockPath)).catch((error: NodeJS.ErrnoException) => {
          if (error.code !== "ENOENT") throw error;
        });
      }
    }
    return "busy";
  }
  try {
    await mkdir(ownerPath);
  } catch (error) {
    await rmdir(lockPath);
    throw error;
  }
  return "acquired";
}

async function updateRemoteTextFile(
  targetPath: string,
  temporaryPath: string,
  update: (content: string) => string | Promise<string>,
  sshHost: string,
  mode?: number,
  options?: Pick<ExecGitOptions, "strictHostKeyChecking">,
): Promise<void> {
  const command = buildRemoteFileUpdateCommand(targetPath, temporaryPath, mode);
  let unownedObservations = 0;
  for (let attempt = 0; attempt < FILE_UPDATE_LOCK_ATTEMPTS; attempt++) {
    let responseStarted = false;
    const output = await execGit(command, sshHost, {
      ...options,
      maxBufferBytes: REMOTE_TEXT_FILE_MAX_BUFFER_BYTES,
      remoteStdinResponder: {
        marker: REMOTE_FILE_UPDATE_READY,
        respond: async (encodedContent) => {
          responseStarted = true;
          const content = Buffer.from(encodedContent, "base64");
          if (content.toString("base64") !== encodedContent) throw new Error("원격 파일 갱신 요청이 손상되었습니다.");
          const nextContent = await update(content.toString("utf-8"));
          return `${Buffer.byteLength(nextContent)}\n${Buffer.from(nextContent, "utf-8").toString("base64")}\n`;
        },
      },
    });
    const records = output.split("\n");
    if (responseStarted && records.includes(REMOTE_FILE_UPDATE_DONE)) return;
    if (responseStarted || records.includes(REMOTE_FILE_UPDATE_DONE)) throw new Error(`원격 파일 갱신 결과를 확인할 수 없습니다: ${targetPath}`);
    if (records.includes(REMOTE_FILE_UPDATE_UNOWNED)) {
      if (++unownedObservations >= FILE_UPDATE_OWNER_INITIALIZATION_ATTEMPTS) {
        throw new Error(`소유자를 확인할 수 없는 파일 잠금입니다. 사용 중인 작업이 없는지 확인한 뒤 잠금을 백업 이동하세요: ${targetPath}.lock`);
      }
    } else if (!records.includes(REMOTE_FILE_UPDATE_BUSY)) {
      throw new Error(`원격 파일 갱신 결과를 확인할 수 없습니다: ${targetPath}`);
    }
    await new Promise((resolve) => setTimeout(resolve, FILE_UPDATE_LOCK_RETRY_MS));
  }
  throw new Error(`파일 잠금 대기 시간이 초과되었습니다: ${targetPath}.lock`);
}

function buildRemoteFileUpdateCommand(targetPath: string, temporaryPath: string, mode?: number): string {
  const lockPath = quoteShellArgument(`${targetPath}.lock`);
  const ownerSuffix = quoteShellArgument(randomUUID());
  const cleanup = `rm -f ${quoteShellArgument(temporaryPath)}; rmdir "$__kanvibe_owner" && rmdir ${lockPath}`;
  return [
    `mkdir -p ${quoteShellArgument(path.posix.dirname(targetPath))} || exit $?;`,
    '__kanvibe_host=$(uname -n | tr -d "\\n" | od -An -tx1 | tr -d " \\n") || exit $?;',
    'test -n "$__kanvibe_host" || exit 1;',
    `if ! mkdir -m 700 ${lockPath} 2>/dev/null; then`,
    `test -d ${lockPath} && ! test -L ${lockPath} || { printf '%s\\n' ${quoteShellArgument(`파일 잠금 경로를 확인하세요: ${targetPath}.lock`)} >&2; exit 1; };`,
    `set -- ${lockPath}/owner-*;`,
    'if [ "$#" = 1 ] && test -d "$1" && ! test -L "$1"; then',
    '__kanvibe_name=${1##*/}; if [ "${__kanvibe_name##*-}" != "$__kanvibe_host" ]; then',
    `printf '%s\\n' ${quoteShellArgument(REMOTE_FILE_UPDATE_UNOWNED)}; exit 0; fi;`,
    '__kanvibe_pid=${__kanvibe_name#owner-}; __kanvibe_pid=${__kanvibe_pid%%-*};',
    'case "$__kanvibe_pid" in ""|0|*[!0-9]*) exit 1;; esac;',
    'if ! kill -0 "$__kanvibe_pid" 2>/dev/null && command -v ps >/dev/null 2>&1; then',
    '__kanvibe_alive=$(ps -p "$__kanvibe_pid" -o pid= 2>/dev/null); __kanvibe_probe_status=$?;',
    `if [ "$__kanvibe_probe_status" = 1 ] && [ -z "$__kanvibe_alive" ]; then rmdir "$1" 2>/dev/null && rmdir ${lockPath}; fi; fi;`,
    `printf '%s\\n' ${quoteShellArgument(REMOTE_FILE_UPDATE_BUSY)};`,
    `else printf '%s\\n' ${quoteShellArgument(REMOTE_FILE_UPDATE_UNOWNED)}; fi; exit 0; fi;`,
    `__kanvibe_owner=${lockPath}/owner-$$-${ownerSuffix}-"$__kanvibe_host";`,
    `mkdir -m 700 "$__kanvibe_owner" || { rmdir ${lockPath}; exit 1; };`,
    `trap ${quoteShellArgument(cleanup)} 0; trap 'exit 1' HUP INT TERM;`,
    `if test -e ${quoteShellArgument(targetPath)} || test -L ${quoteShellArgument(targetPath)}; then`,
    `test -f ${quoteShellArgument(targetPath)} || exit 1;`,
    `__kanvibe_current=$(base64 -w 0 ${quoteShellArgument(targetPath)} 2>/dev/null || base64 < ${quoteShellArgument(targetPath)}) || exit $?;`,
    'else __kanvibe_current=""; fi;',
    `printf '%s\\t%s\\n' ${quoteShellArgument(REMOTE_FILE_UPDATE_READY)} "$(printf '%s' "$__kanvibe_current" | tr -d '\\n')";`,
    'IFS= read -r __kanvibe_length && IFS= read -r __kanvibe_next || exit 1;',
    'case "$__kanvibe_length" in ""|*[!0-9]*) exit 1;; esac;',
    'test -d "$__kanvibe_owner" && ! test -L "$__kanvibe_owner" || exit 1;',
    `printf '%s' "$__kanvibe_next" | ${buildRemoteStdinPublishCommand(temporaryPath, targetPath, "$__kanvibe_length", mode)} || exit $?;`,
    `printf '%s\\n' ${quoteShellArgument(REMOTE_FILE_UPDATE_DONE)}`,
  ].join(" ");
}

/**
 * 표준 입력의 base64를 임시 파일에 풀어 대상 파일로 교체하는 명령을 만든다.
 * 내용을 명령줄에 싣지 않아 `ps`와 프로세스 감사 로그에 자격 증명이 남지 않는다.
 * 로그인 프로파일이 입력을 먼저 소비하면 빈 파일이 생기므로, 크기가 맞을 때만 교체해 기존 내용을 잃지 않는다.
 */
function buildRemoteStdinPublishCommand(
  temporaryPath: string,
  targetPath: string,
  expectedByteLength: number | string,
  mode?: number,
): string {
  const quotedTemporaryPath = quoteShellArgument(temporaryPath);
  const chmodCommand = mode !== undefined ? `chmod ${mode.toString(8)} ${quotedTemporaryPath} && ` : "";
  return `(${buildRemoteUmaskCommand(mode)}(base64 -d 2>/dev/null || base64 -D) > ${quotedTemporaryPath})`
    + ` && [ "$(wc -c < ${quotedTemporaryPath} | tr -d ' ')" = "${expectedByteLength}" ]`
    + ` && ${chmodCommand}mv -f ${quotedTemporaryPath} ${quoteShellArgument(targetPath)}`;
}

/**
 * 대상 파일이 아직 없을 때만 기록한다.
 * 이미 존재하는 파일은 다른 주체가 확정한 값이므로 검사와 기록 사이에 끼어든 쓰기도 덮지 않아야 한다.
 * 그래서 로컬은 `wx` 플래그로, 원격은 no-clobber redirection으로 생성 시 경합을 막는다.
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
    `mkdir -p ${quoteShellArgument(path.posix.dirname(targetPath))} && if [ -e ${quotedTargetPath} ]; then :; else (set -C; printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quotedTargetPath}) || test -e ${quotedTargetPath}; fi`,
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
