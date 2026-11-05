import { afterEach, describe, expect, it, vi } from "vitest";
import { getFileMtimeMs } from "@/lib/hostFileAccess";
import { getCachedOrParse, getCachedOrParseHead } from "@/lib/aiSessions/shared";

vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/hostFileAccess")>(),
  getFileMtimeMs: vi.fn(),
}));

afterEach(() => vi.restoreAllMocks());

describe.each([
  ["full", getCachedOrParse],
  ["head", getCachedOrParseHead],
] as const)("%s parsing cache freshness", (kind, getParsed) => {
  it("eventually reparses a remote last write with unchanged coarse mtime", async () => {
    const clock = vi.spyOn(Date, "now").mockReturnValue(10_000);
    vi.mocked(getFileMtimeMs).mockResolvedValue(1_000);
    const parse = vi.fn().mockResolvedValueOnce("before").mockResolvedValueOnce("after");
    const file = `/remote/${kind}-same-second.jsonl`;
    expect(await getParsed(file, parse, "fixture-host")).toBe("before");
    clock.mockReturnValue(10_500);
    expect(await getParsed(file, parse, "fixture-host")).toBe("before");
    clock.mockReturnValue(11_001);
    expect(await getParsed(file, parse, "fixture-host")).toBe("after");
    expect(parse).toHaveBeenCalledTimes(2);
  });

  it("revalidates each remote coarse mtime once without repeatedly downloading settled history", async () => {
    const clock = vi.spyOn(Date, "now").mockReturnValue(20_000);
    vi.mocked(getFileMtimeMs).mockResolvedValue(2_000);
    const parse = vi.fn().mockResolvedValue("large-history");
    const file = `/remote/${kind}-settled.jsonl`;
    await getParsed(file, parse, "fixture-host");
    clock.mockReturnValue(21_001);
    await getParsed(file, parse, "fixture-host");
    clock.mockReturnValue(30_000);
    await getParsed(file, parse, "fixture-host");
    clock.mockReturnValue(600_000);
    expect(await getParsed(file, parse, "fixture-host")).toBe("large-history");
    expect(parse).toHaveBeenCalledTimes(2);

    vi.mocked(getFileMtimeMs).mockResolvedValue(3_000);
    parse.mockResolvedValue("new-record");
    expect(await getParsed(file, parse, "fixture-host")).toBe("new-record");
    expect(parse).toHaveBeenCalledTimes(3);
    clock.mockReturnValue(601_001);
    await getParsed(file, parse, "fixture-host");
    clock.mockReturnValue(900_000);
    await getParsed(file, parse, "fixture-host");
    expect(parse).toHaveBeenCalledTimes(4);
  });

  it("keeps unchanged local files cached and invalidates changed mtimes immediately", async () => {
    const clock = vi.spyOn(Date, "now").mockReturnValue(10_000);
    vi.mocked(getFileMtimeMs).mockResolvedValue(1_000);
    const parse = vi.fn().mockResolvedValueOnce("before").mockResolvedValueOnce("after");
    const file = `/local/${kind}-precise.jsonl`;
    expect(await getParsed(file, parse)).toBe("before");
    clock.mockReturnValue(50_000);
    expect(await getParsed(file, parse)).toBe("before");
    vi.mocked(getFileMtimeMs).mockResolvedValue(1_001);
    expect(await getParsed(file, parse)).toBe("after");
    expect(parse).toHaveBeenCalledTimes(2);
  });
});
