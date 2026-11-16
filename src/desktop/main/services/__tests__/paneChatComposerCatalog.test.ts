import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { readPaneChatSuggestions } from "../paneChatComposerCatalog";

const remoteExec = vi.hoisted(() => vi.fn());
vi.mock("@/lib/gitOperations", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/gitOperations")>();
  return { ...actual, execGit: (...args: Parameters<typeof actual.execGit>) => args[1] ? remoteExec(...args) : actual.execGit(...args) };
});

let directory: string;
beforeEach(async () => { vi.clearAllMocks(); directory = await mkdtemp(path.join(tmpdir(), "kv-catalog-")); });
afterEach(async () => { await rm(directory, { recursive: true, force: true }); });
async function skill(root: string, name: string, body: string) {
  const target = path.join(root, name);
  await mkdir(target, { recursive: true });
  await writeFile(path.join(target, "SKILL.md"), body);
}
describe("pane chat composer catalog", () => {
  it("prefers project skills, follows provider symlinks and skips one malformed skill", async () => {
    const home = path.join(directory, "home"), cwd = path.join(directory, "repo");
    await skill(path.join(home, ".agents/skills"), "review", "---\nname: review\ndescription: Home review\n---\n");
    await skill(path.join(cwd, ".agents/skills"), "review", "---\nname: review\ndescription: Project review\n---\n");
    await skill(path.join(cwd, ".agents/skills"), "bad", "---\nname: [invalid\n---\n");
    await skill(path.join(directory, "external"), "linked", "---\nname: linked\ndescription: Linked skill\n---\n");
    await symlink(path.join(directory, "external/linked"), path.join(cwd, ".agents/skills/linked"));
    const result = await readPaneChatSuggestions({ provider: "codex", cwd, homeDirectory: home }, "skill", "");
    expect(result.items.map((entry) => entry.label)).toEqual(["linked", "review"]);
    expect(result.items.find((entry) => entry.label === "review")).toMatchObject({ description: "Project review", insertText: "$review" });
  });
  it("uses Claude slash commands and hides non user-invocable skills", async () => {
    const home = path.join(directory, "home"), cwd = path.join(directory, "repo");
    await skill(path.join(cwd, ".claude/skills"), "visible", "---\nname: visible\ndescription: Visible\n---\n");
    await skill(path.join(cwd, ".claude/skills"), "hidden", "---\nname: hidden\nuser-invocable: false\n---\n");
    await mkdir(path.join(cwd, ".claude/commands"), { recursive: true });
    await writeFile(path.join(cwd, ".claude/commands/check.md"), "---\ndescription: Check files\n---\nRun checks");
    const skills = await readPaneChatSuggestions({ provider: "claude", cwd, homeDirectory: home }, "skill", "");
    expect(skills.items.map((entry) => entry.insertText)).toEqual(["/visible"]);
    const commands = await readPaneChatSuggestions({ provider: "claude", cwd, homeDirectory: home }, "command", "check");
    expect(commands.items).toEqual([expect.objectContaining({ label: "check", insertText: "/check" })]);
  });
  it("suggests repo files with safe mentions while respecting git ignores", async () => {
    const cwd = path.join(directory, "repo");
    await mkdir(cwd);
    execFileSync("git", ["init", "-q", cwd]);
    await writeFile(path.join(cwd, "design notes.md"), "notes");
    await writeFile(path.join(cwd, ".gitignore"), "secret.txt\n");
    await writeFile(path.join(cwd, "secret.txt"), "ignored");
    await writeFile(path.join(cwd, " design leading.md"), "leading space");
    const result = await readPaneChatSuggestions({ provider: "codex", cwd, homeDirectory: directory }, "file", "design notes");
    expect(result.items).toEqual([expect.objectContaining({ label: "design notes.md", insertText: `@"${cwd}/design notes.md"` })]);
    expect((await readPaneChatSuggestions({ provider: "codex", cwd, homeDirectory: directory }, "file", "secret")).items).toEqual([]);
    expect((await readPaneChatSuggestions({ provider: "codex", cwd, homeDirectory: directory }, "file", "leading")).items[0]).toMatchObject({ label: " design leading.md", insertText: `@"${cwd}/ design leading.md"` });
  });
  it("uses the selected CLI process's custom provider home without scanning another account", async () => {
    const home = path.join(directory, "home"), custom = path.join(directory, "custom provider home"), cwd = path.join(directory, "repo");
    await skill(path.join(custom, "skills"), "custom", "---\nname: custom\ndescription: Custom home\n---\n");
    const child = spawn(process.execPath, ["-e", "setTimeout(() => {}, 10000)"], { env: { ...process.env, CODEX_HOME: custom }, stdio: "ignore" });
    try {
      await once(child, "spawn");
      const result = await readPaneChatSuggestions({ provider: "codex", cwd, homeDirectory: home, processId: child.pid }, "skill", "custom");
      expect(result.items).toEqual([expect.objectContaining({ label: "custom", insertText: "$custom" })]);
    } finally { child.kill(); }
  });
  it("reads remote skill bytes only through the selected SSH host", async () => {
    const skillPath = "/remote/repo/.agents/skills/remote/SKILL.md";
    const content = "---\nname: remote\ndescription: Remote only\n---\n";
    remoteExec.mockImplementation(async (command: string) => command.startsWith("for __kanvibe_skill") ? `${skillPath}\0${Buffer.from(content).toString("base64")}\0` : command.includes("'/remote/repo/.agents/skills'") ? `${skillPath}\0` : "");
    const result = await readPaneChatSuggestions({ provider: "codex", cwd: "/remote/repo", homeDirectory: "/remote/home", providerHome: "/remote/home/.codex", sshHost: "fixture-host" }, "skill", "remote");
    expect(result.items).toEqual([expect.objectContaining({ label: "remote", insertText: "$remote" })]);
    expect(remoteExec.mock.calls.every((call) => call[1] === "fixture-host")).toBe(true);
  });
  it("bounds visible suggestions and announces that more matches exist", async () => {
    const cwd = path.join(directory, "repo");
    for (let index = 0; index < 21; index++) await skill(path.join(cwd, ".agents/skills"), `skill-${index}`, `---\nname: skill-${index}\ndescription: Result\n---\n`);
    const result = await readPaneChatSuggestions({ provider: "codex", cwd, homeDirectory: directory }, "skill", "skill-");
    expect(result.items).toHaveLength(20);
    expect(result.truncated).toBe(true);
  });
  it("returns the whole catalog when the caller raises the limit to filter on its own side", async () => {
    const cwd = path.join(directory, "repo");
    for (let index = 0; index < 21; index++) await skill(path.join(cwd, ".agents/skills"), `skill-${index}`, `---\nname: skill-${index}\ndescription: Result\n---\n`);
    const result = await readPaneChatSuggestions({ provider: "codex", cwd, homeDirectory: directory }, "skill", "", 512);
    expect(result.items).toHaveLength(21);
    expect(result.truncated).toBe(false);
  });
  it("reuses a scanned catalog for ten seconds and then picks up a newly added skill", async () => {
    const cwd = path.join(directory, "repo");
    const context = { provider: "codex" as const, cwd, homeDirectory: directory };
    const labels = async () => (await readPaneChatSuggestions(context, "skill", "")).items.map((entry) => entry.label);
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      await skill(path.join(cwd, ".agents/skills"), "first", "---\nname: first\ndescription: First\n---\n");
      expect(await labels()).toEqual(["first"]);
      await skill(path.join(cwd, ".agents/skills"), "second", "---\nname: second\ndescription: Second\n---\n");
      vi.advanceTimersByTime(9_999);
      expect(await labels()).toEqual(["first"]);
      vi.advanceTimersByTime(1);
      expect(await labels()).toEqual(["first", "second"]);
    } finally { vi.useRealTimers(); }
  });
  it("shares one scan between requests that arrive while it is still running", async () => {
    const context = { provider: "codex" as const, cwd: "/remote/repo", homeDirectory: "/remote/home", providerHome: "/remote/home/.codex", sshHost: "shared-scan-host" };
    remoteExec.mockResolvedValue("");
    await Promise.all([readPaneChatSuggestions(context, "skill", ""), readPaneChatSuggestions(context, "command", "")]);
    const scannedRoots = remoteExec.mock.calls.filter((call) => String(call[0]).includes("'/remote/home/.codex/skills'"));
    expect(scannedRoots).toHaveLength(1);
  });
  it("does not remember a failed scan", async () => {
    const context = { provider: "codex" as const, cwd: "/remote/repo", homeDirectory: "/remote/home", providerHome: "/remote/home/.codex", sshHost: "failing-scan-host" };
    remoteExec.mockRejectedValueOnce(new Error("ssh unavailable"));
    await expect(readPaneChatSuggestions(context, "skill", "")).rejects.toThrow("ssh unavailable");
    remoteExec.mockResolvedValue("");
    await expect(readPaneChatSuggestions(context, "skill", "")).resolves.toEqual({ items: [], truncated: false });
  });
});
