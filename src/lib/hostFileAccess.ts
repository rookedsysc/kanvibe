import { access, mkdir, readFile, readdir, stat, writeFile } from "fs/promises";
import { homedir } from "os";
import path from "path";
import { execGit } from "@/lib/gitOperations";

const remoteHomeDirectoryCache = new Map<string, string>();
const REMOTE_FILE_RECORD_PREFIX = "__KANVIBE_FILE_RECORD__";

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

  try {
    return await execGit(
      `test -f ${quoteShellArgument(targetPath)} && cat ${quoteShellArgument(targetPath)} || true`,
      sshHost,
    );
  } catch {
    return "";
  }
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

  const command = [
    "for __kanvibe_file in",
    targetPaths.map(quoteShellArgument).join(" "),
    "; do",
    `printf '%s\\t%s\\t' ${quoteShellArgument(REMOTE_FILE_RECORD_PREFIX)} "$__kanvibe_file";`,
    "if test -f \"$__kanvibe_file\"; then",
    "printf '1\\t';",
    "(base64 -w 0 \"$__kanvibe_file\" 2>/dev/null || base64 \"$__kanvibe_file\" | tr -d '\\n');",
    "else",
    "printf '0\\t';",
    "fi;",
    "printf '\\n';",
    "done",
  ].join(" ");
  const output = await execGit(command, sshHost);

  return parseRemoteTextFiles(output, targetPaths);
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
