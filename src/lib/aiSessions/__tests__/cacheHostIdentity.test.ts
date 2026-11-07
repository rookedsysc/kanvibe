import { afterEach, describe, expect, it, vi } from "vitest";

const stat = vi.hoisted(() => vi.fn(async () => 123));
const execute = vi.hoisted(() => vi.fn(async () => '{"source":"head"}\n'));
vi.mock("@/lib/hostFileAccess", () => ({
  getFileMtimeMs: stat,
  quoteShellArgument: (value: string) => `'${value.replaceAll("'", "'\\''")}'`,
  readTextFile: vi.fn(async () => '{"source":"full"}\n'),
}));
vi.mock("@/lib/gitOperations", () => ({ execGit: execute }));
afterEach(() => vi.clearAllMocks());

describe("host-specific JSONL reads", () => {
  it.each(["getCachedOrParse", "getCachedOrParseHead"] as const)("isolates local and remote cache entries for %s", async (method) => {
    const shared = await import("@/lib/aiSessions/shared");
    const file = `/same/${method}.jsonl`;
    for (const host of [null, "host-a", "host-b"]) {
      expect(await shared[method](file, async () => host ?? "local", host)).toBe(host ?? "local");
      expect(stat).toHaveBeenLastCalledWith(file, host);
    }
    expect(await shared[method](file, async () => "wrong", "host-a")).toBe("host-a");
  });

  it("reads only requested head lines on the remote host with a quoted path", async () => {
    const { readJsonLinesHead } = await import("@/lib/aiSessions/shared");
    expect(await readJsonLinesHead("/tmp/a'b.jsonl", 1, "remote")).toEqual([{ source: "head" }]);
    expect(execute).toHaveBeenCalledWith("head -n 1 '/tmp/a'\\''b.jsonl'", "remote");
  });
});
