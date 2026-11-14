// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { chmod, lstat, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { openRemoteHostHookJournal, HOST_HOOK_JOURNAL_MAX_EVENTS, HOST_HOOK_JOURNAL_MAX_BYTES, HOST_HOOK_JOURNAL_RETENTION_MS } from "../remoteHostHookJournal";

describe("durable host hook journal", () => {
  let rootDirectory: string;
  beforeEach(async () => { rootDirectory = await mkdtemp("/tmp/kvhj-"); });
  afterEach(async () => { vi.restoreAllMocks(); await rm(rootDirectory, { recursive: true, force: true }); });
  const request = (status = "progress") => ({ path: "/api/hooks/status", method: "POST", headers: { "content-type": "application/json" }, body: Buffer.from(JSON.stringify({ taskId: "task-one", status })).toString("base64") });
  const open = () => openRemoteHostHookJournal({ namespace: "qa-journal", rootDirectory });

  it("durably restores ordered events and preserves client isolation", async () => {
    const journal = await open();
    const a = await journal.append("client-a", request());
    const b = await journal.append("client-a", request("done"));
    const other = await journal.append("client-b", request("review"));
    expect(a.id).not.toBe(b.id);
    expect(b.sequence).toBeGreaterThan(a.sequence);
    const restored = await open();
    expect(await restored.next("client-a")).toEqual(a);
    expect(await restored.next("client-b")).toEqual(other);
    expect((await lstat(restored.filePath)).mode & 0o777).toBe(0o600);
    expect((await lstat(path.dirname(restored.filePath))).mode & 0o777).toBe(0o700);
  });

  it("keeps lost acknowledgements and removes only the client's oldest acknowledged event", async () => {
    const journal = await open();
    const a = await journal.append("client-a", request());
    const b = await journal.append("client-a", request("done"));
    expect(await journal.acknowledge("client-b", a.id, "accepted")).toBe(false);
    expect(await journal.acknowledge("client-a", b.id, "accepted")).toBe(false);
    expect(await (await open()).next("client-a")).toEqual(a);
    expect(await journal.acknowledge("client-a", a.id, "accepted")).toBe(true);
    expect(await journal.acknowledge("client-a", a.id, "accepted")).toBe(false);
    expect(await (await open()).next("client-a")).toEqual(b);
    await journal.acknowledge("client-a", b.id, "rejected");
    expect(await (await open()).status()).toMatchObject({ pending: 0, rejected: 1 });
  });

  it("serializes concurrent append and acknowledge without losing records", async () => {
    const journal = await open();
    const records = await Promise.all(Array.from({ length: 20 }, (_, i) => journal.append(i % 2 ? "client-a" : "client-b", request())));
    expect(new Set(records.map(e => e.sequence)).size).toBe(20);
    expect((await (await open()).status()).pending).toBe(20);
    const first = await journal.next("client-a");
    await Promise.all([journal.acknowledge("client-a", first!.id, "accepted"), journal.append("client-a", request())]);
    expect((await (await open()).status()).pending).toBe(20);
  });

  it("expires records with a persisted counter and permits a later append", async () => {
    const clock = vi.spyOn(Date, "now").mockReturnValue(1_800_000_000_000);
    const journal = await open();
    await journal.append("client-a", request());
    clock.mockReturnValue(1_800_000_000_000 + HOST_HOOK_JOURNAL_RETENTION_MS + 1);
    expect(await journal.next("client-a")).toBeNull();
    expect(await (await open()).status()).toMatchObject({ pending: 0, expired: 1 });
    await journal.append("client-a", request("done"));
    expect((await journal.status()).pending).toBe(1);
  });

  it("rejects count capacity without evicting the existing queue", async () => {
    const journal = await open();
    const first = await journal.append("client-a", request());
    const disk = JSON.parse(await readFile(journal.filePath, "utf8"));
    disk.events = Array.from({ length: HOST_HOOK_JOURNAL_MAX_EVENTS }, (_, i) => ({ ...first, id: "event-" + i, sequence: i + 1 }));
    disk.nextSequence = HOST_HOOK_JOURNAL_MAX_EVENTS + 1;
    await writeFile(journal.filePath, JSON.stringify(disk), { mode: 0o600 });
    const restored = await open();
    await expect(restored.append("client-a", request())).rejects.toThrow(/capacity/i);
    expect(await (await open()).status()).toMatchObject({ pending: HOST_HOOK_JOURNAL_MAX_EVENTS, rejected: 1 });
    expect((await restored.next("client-a"))!.id).toBe("event-0");
  });

  it("rejects byte capacity and exposes a persisted rejection counter", async () => {
    const journal = await open();
    const payload = { ...request(), body: Buffer.from(JSON.stringify({ taskId: "task-one", status: "progress", extra: "x".repeat(490_000) })).toString("base64") };
    let accepted = 0;
    for (let i = 0; i < 40; i++) {
      try { await journal.append("client-a", payload); accepted++; } catch (error) { expect(String(error)).toMatch(/capacity/); break; }
    }
    expect(accepted).toBeGreaterThan(0);
    expect(accepted).toBeLessThan(40);
    const status = await (await open()).status();
    expect(status.pending).toBe(accepted);
    expect(status.bytes).toBeLessThanOrEqual(HOST_HOOK_JOURNAL_MAX_BYTES);
    expect(status.rejected).toBe(1);
  });

  it.each([
    ["bad/client", request()],
    ["client-a", { ...request(), path: "/api/cli/tasks" }],
    ["client-a", { ...request(), method: "GET" }],
    ["client-a", { ...request(), body: Buffer.from("{}").toString("base64") }],
    ["client-a", { ...request(), headers: { origin: "https://example.com" } }],
  ])("rejects invalid status events %s", async (clientId, event) => {
    const journal = await open();
    await expect(journal.append(clientId, event)).rejects.toThrow();
    expect((await journal.status()).pending).toBe(0);
  });

  it("rejects a single oversized event before writing and keeps the pending queue", async () => {
    const journal = await open();
    const first = await journal.append("client-a", request());
    const body = Buffer.from(JSON.stringify({ taskId: "task-one", status: "progress", extra: "x".repeat(512 * 1024) })).toString("base64");
    await expect(journal.append("client-a", { ...request(), body })).rejects.toThrow(/bytes|size/i);
    expect(await (await open()).next("client-a")).toEqual(first);
  });

  it("rejects unsafe directories, symlink journal files and corrupt snapshots", async () => {
    await chmod(rootDirectory, 0o755);
    await expect(open()).rejects.toThrow(/private/i);
    await chmod(rootDirectory, 0o700);
    const journal = await open();
    await writeFile(path.join(rootDirectory, "foreign"), "{}");
    await symlink(path.join(rootDirectory, "foreign"), journal.filePath);
    await expect(open()).rejects.toThrow(/trusted|journal/i);
    await rm(journal.filePath);
    await writeFile(journal.filePath, "{broken", { mode: 0o600 });
    await expect(open()).rejects.toThrow();
    expect(await readFile(journal.filePath, "utf8")).toBe("{broken");
  });

  it("removes only verified abandoned publication files after recovering a valid journal", async () => {
    const journal = await open();
    const event = await journal.append("client-a", request());
    const directory = path.dirname(journal.filePath);
    const pending = path.join(directory, ".hook-journal-11111111-1111-4111-8111-111111111111.tmp");
    const foreign = path.join(directory, ".hook-journal-not-ours.tmp");
    await writeFile(pending, "incomplete publication", { mode: 0o600 });
    await writeFile(foreign, "preserve", { mode: 0o600 });
    expect(await (await open()).next("client-a")).toEqual(event);
    await expect(lstat(pending)).rejects.toMatchObject({ code: "ENOENT" });
    expect(await readFile(foreign, "utf8")).toBe("preserve");
    const link = path.join(directory, ".hook-journal-22222222-2222-4222-8222-222222222222.tmp");
    await symlink(journal.filePath, link);
    await expect(open()).rejects.toThrow(/publication|trusted/i);
    expect(await readFile(journal.filePath, "utf8")).toContain(event.id);
    expect((await lstat(link)).isSymbolicLink()).toBe(true);
  });

  it("leaves prior durable state intact when publishing a snapshot fails", async () => {
    const journal = await open();
    const first = await journal.append("client-a", request());
    const previous = await readFile(journal.filePath, "utf8");
    await chmod(path.dirname(journal.filePath), 0o500);
    try { await expect(journal.append("client-a", request("done"))).rejects.toThrow(); }
    finally { await chmod(path.dirname(journal.filePath), 0o700); }
    expect(await readFile(journal.filePath, "utf8")).toBe(previous);
    expect(await journal.next("client-a")).toEqual(first);
  });
});
