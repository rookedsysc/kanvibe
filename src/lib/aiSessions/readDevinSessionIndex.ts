import { execFile } from "child_process";
import { promisify } from "util";
import { execGit } from "@/lib/gitOperations";
import { pathExists, quoteShellArgument } from "@/lib/hostFileAccess";

const execFileAsync = promisify(execFile);
const INDEX_TIMEOUT_MS = 5_000;
const INDEX_MAX_BUFFER_BYTES = 4 * 1024 * 1024;
const READ_DEVIN_INDEX = `
import json, sqlite3, sys
from urllib.parse import quote
connection = sqlite3.connect("file:" + quote(sys.argv[1], safe="/") + "?mode=ro", uri=True, timeout=2)
try:
    connection.row_factory = sqlite3.Row
    columns = {row[1] for row in connection.execute("PRAGMA table_info(sessions)")}
    if "id" not in columns:
        print("[]")
    else:
        selected = [name for name in ["id", "working_directory", "title", "created_at", "last_activity_at", "hidden"] if name in columns]
        print(json.dumps([dict(row) for row in connection.execute("SELECT " + ",".join(selected) + " FROM sessions")]))
finally:
    connection.close()
`;

// DB와 WAL의 별도 캐시 대신 조회 한 번 안에서만 공유하여 hidden·cwd 변경을 즉시 반영한다.
export async function readDevinSessionIndex(dbPath: string, sshHost?: string | null): Promise<Map<string, Record<string, unknown>>> {
  if (!await pathExists(dbPath, sshHost)) return new Map();
  const output = sshHost
    ? await execGit(`python3 -c ${quoteShellArgument(READ_DEVIN_INDEX)} ${quoteShellArgument(dbPath)}`, sshHost, { maxBufferBytes: INDEX_MAX_BUFFER_BYTES })
    : (await execFileAsync("python3", ["-c", READ_DEVIN_INDEX, dbPath], { timeout: INDEX_TIMEOUT_MS, maxBuffer: INDEX_MAX_BUFFER_BYTES })).stdout;
  const rows = JSON.parse(output) as Array<Record<string, unknown>>;
  return new Map(rows.filter((row) => typeof row.id === "string").map((row) => [row.id as string, row]));
}
