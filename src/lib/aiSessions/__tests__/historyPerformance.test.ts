import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  mtime: vi.fn(),
  execGit: vi.fn(),
  readTextFile: vi.fn(),
  listFiles: vi.fn(),
}));

vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execGit }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/hostFileAccess")>(),
  getFileMtimeMs: mocks.mtime,
  readTextFile: mocks.readTextFile,
  getHomeDirectory: async () => "/remote/home",
  pathExists: async () => true,
  listFilesRecursivelyBySuffix: mocks.listFiles,
}));

import { getCachedOrParse, getCachedOrParseHead, readJsonLinesHead } from "@/lib/aiSessions/shared";

import { readCodexSessions } from "@/lib/aiSessions/readCodexSessions";

describe("History I/O", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.mtime.mockResolvedValue(1);
  });

  it.each([getCachedOrParse, getCachedOrParseHead])("isolates identical file paths by execution host (%#)", async (cache) => {
    const file = `/same/path/host-${cache.name}.jsonl`;
    const local = await cache(file, async () => "local");
    const firstHost = await cache(file, async () => "host-a", "host-a");
    const secondHost = await cache(file, async () => "host-b", "host-b");
    expect([local, firstHost, secondHost]).toEqual(["local", "host-a", "host-b"]);
  });

  it.each([getCachedOrParse, getCachedOrParseHead])("shares concurrent reads of the same unchanged file (%#)", async (cache) => {
    const parse = vi.fn(async () => "parsed");
    const file = `/concurrent/${cache.name}.jsonl`;
    expect(await Promise.all([cache(file, parse), cache(file, parse)])).toEqual(["parsed", "parsed"]);
    expect(parse).toHaveBeenCalledTimes(1);
  });

  it("excludes unrelated Codex rollouts before loading message bodies", async () => {
    const file = "/remote/foreign.jsonl";
    const meta = { type: "session_meta", payload: { id: "foreign", cwd: "/another/worktree" } };
    mocks.listFiles.mockResolvedValue([file]);
    mocks.execGit.mockResolvedValue(JSON.stringify(meta));
    mocks.readTextFile.mockResolvedValue(JSON.stringify(meta));
    const result = await readCodexSessions({ worktreePath: "/selected/worktree", repoPath: "/selected/worktree", sshHost: "foreign-host" });
    expect(result.sessions).toEqual([]);
    expect(mocks.readTextFile).not.toHaveBeenCalled();
  });

  it("only transfers the requested remote header lines", async () => {
    mocks.execGit.mockResolvedValue('{"type":"session_meta"}\n');
    mocks.readTextFile.mockResolvedValue('{"type":"session_meta"}\n');
    expect(await readJsonLinesHead("/remote/history.jsonl", 1, "remote")).toEqual([{ type: "session_meta" }]);
    expect(mocks.execGit).toHaveBeenCalledWith("head -n 1 '/remote/history.jsonl'", "remote");
    expect(mocks.readTextFile).not.toHaveBeenCalled();
  });
});
