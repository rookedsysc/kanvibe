import { execFile } from "child_process";
import { mkdir, mkdtemp, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClaudeSessionDetail, readClaudeSessions } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexSessionDetail, readCodexSessions } from "@/lib/aiSessions/readCodexSessions";
import { readGeminiSessionDetail, readGeminiSessions } from "@/lib/aiSessions/readGeminiSessions";

let tempHome: string;

async function writeJson(filePath: string, value: unknown) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf-8");
}

async function writeJsonLines(filePath: string, values: unknown[]) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, jsonLines(values), "utf-8");
}

function jsonLines(values: unknown[]) {
  return `${values.map((value) => JSON.stringify(value)).join("\n")}\n`;
}

function claudeProjectDirectoryName(targetPath: string) {
  return path.resolve(targetPath).replaceAll(path.sep, "-").replaceAll("_", "-");
}

function setupRemoteFileSystem(files: Record<string, string>) {
  const fileMap = new Map(Object.entries(files));
  const directorySet = new Set<string>();
  const execCalls: Array<{ command: string; sshHost?: string | null; options?: { maxBufferBytes?: number } }> = [];

  for (const filePath of fileMap.keys()) {
    let current = path.dirname(filePath);
    while (current && current !== path.dirname(current)) {
      directorySet.add(current);
      current = path.dirname(current);
    }
  }

  const pathExists = (targetPath: string) => fileMap.has(targetPath) || directorySet.has(targetPath);
  const listFiles = (rootPath: string, suffix: string, recursive: boolean) => Array.from(fileMap.keys())
    .filter((filePath) => filePath.endsWith(suffix))
    .filter((filePath) => recursive
      ? filePath.startsWith(`${rootPath}/`)
      : path.dirname(filePath) === rootPath)
    .sort()
    .join("\n");

  vi.doMock("@/lib/gitOperations", () => ({
    execGit: vi.fn(async (command: string, sshHost?: string | null, options?: { maxBufferBytes?: number }) => {
      execCalls.push({ command, sshHost, options });
      if (command === "printf '%s' \"$HOME\"") {
        return "/remote/home";
      }

      const findMatch = command.match(/test -d '([^']+)' && find '([^']+)' (?:-maxdepth 1 )?-type f -name '\*([^']+)'/);
      if (findMatch) {
        if (findMatch[1] !== findMatch[2]) {
          throw new Error(`find command path mismatch: ${command}`);
        }
        return listFiles(findMatch[1], findMatch[3], !command.includes(" -maxdepth 1 "));
      }

      const catMatch = command.match(/test -f '([^']+)' && cat '([^']+)' \|\| true/);
      if (catMatch) {
        if (catMatch[1] !== catMatch[2]) {
          throw new Error(`cat command path mismatch: ${command}`);
        }
        return fileMap.get(catMatch[1]) ?? "";
      }

      const tailMatch = command.match(/tail -n (\d+) '([^']+)'/);
      if (tailMatch) {
        return (fileMap.get(tailMatch[2]) ?? "").split("\n").slice(-Number(tailMatch[1]) - 1).join("\n");
      }

      const statMatch = command.match(/test -e '([^']+)' && \(stat -c %Y '([^']+)'/);
      if (statMatch) {
        if (statMatch[1] !== statMatch[2]) {
          throw new Error(`stat command path mismatch: ${command}`);
        }
        return pathExists(statMatch[1]) ? "1700000000" : "";
      }

      const existsMatch = command.match(/test -e '([^']+)' && printf '1' \|\| true/);
      if (existsMatch) {
        return pathExists(existsMatch[1]) ? "1" : "";
      }

      throw new Error(`unexpected remote command: ${command}`);
    }),
  }));

  return { execCalls };
}

describe("AI session history readers", () => {
  beforeEach(async () => {
    tempHome = await mkdtemp(path.join(tmpdir(), "kanvibe-ai-sessions-"));
    vi.stubEnv("HOME", tempHome);
  });

  afterEach(async () => {
    vi.doUnmock("@/lib/sqliteConnectionPool");
    vi.doUnmock("@/lib/gitOperations");
    vi.resetModules();
    vi.unstubAllEnvs();
    await rm(tempHome, { recursive: true, force: true });
  });

  it("classifies Claude JSONL user text, system input, assistant answers, and tool results separately", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const sessionFile = path.join(
      tempHome,
      ".claude",
      "projects",
      claudeProjectDirectoryName(worktreePath),
      "claude-session.jsonl",
    );

    await writeJsonLines(sessionFile, [
      {
        type: "system",
        sessionId: "claude-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:00:00.000Z",
        message: { role: "system", content: "You are Claude Code." },
      },
      {
        type: "user",
        sessionId: "claude-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:01:00.000Z",
        message: { role: "user", content: [{ type: "text", text: "fix the bug" }] },
      },
      {
        type: "assistant",
        sessionId: "claude-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:02:00.000Z",
        message: { role: "assistant", content: [{ type: "text", text: "I fixed it." }] },
      },
      {
        type: "user",
        sessionId: "claude-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:03:00.000Z",
        message: { role: "user", content: [{ type: "tool_result", content: "file contents" }] },
      },
    ]);

    const sessions = await readClaudeSessions({ worktreePath, repoPath: worktreePath });
    expect(sessions.sessions[0]?.updatedAt).toBe("2026-01-01T00:02:00.000Z");

    const detail = await readClaudeSessionDetail(
      { worktreePath, repoPath: worktreePath },
      "claude-session",
      sessionFile,
      null,
      20,
    );

    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["tool", "file contents"],
      ["assistant", "I fixed it."],
      ["user", "fix the bug"],
      ["system", "You are Claude Code."],
    ]);
  });

  it("classifies Codex JSONL developer instructions, user input, reasoning, tool events, and assistant answers", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const sessionFile = path.join(tempHome, ".codex", "sessions", "2026", "codex-session.jsonl");

    await writeJsonLines(sessionFile, [
      {
        type: "session_meta",
        timestamp: "2026-01-01T00:00:00.000Z",
        payload: { id: "codex-session", cwd: worktreePath, timestamp: "2026-01-01T00:00:00.000Z" },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:01:00.000Z",
        payload: { type: "message", role: "developer", content: [{ type: "input_text", text: "Follow project rules." }] },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:02:00.000Z",
        payload: { type: "message", role: "user", content: [{ type: "input_text", text: "implement history view" }] },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:03:00.000Z",
        payload: { type: "reasoning", text: "Need to inspect schemas." },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:04:00.000Z",
        payload: { type: "function_call", name: "read_file", arguments: "{\"path\":\"x\"}" },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:05:00.000Z",
        payload: { type: "function_call_output", output: "file text" },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:06:00.000Z",
        payload: { type: "message", role: "assistant", content: [{ type: "output_text", text: "Done." }] },
      },
    ]);

    const detail = await readCodexSessionDetail(
      { worktreePath, repoPath: worktreePath },
      "codex-session",
      sessionFile,
      null,
      20,
    );

    expect(detail?.messages.map((message) => message.role)).toEqual([
      "assistant",
      "tool",
      "tool",
      "reasoning",
      "user",
      "developer",
    ]);
    expect(detail?.messages.find((message) => message.role === "developer")?.fullText).toBe("Follow project rules.");
  });

  it("uses actual Codex exec prompts and final answers instead of internal environment messages", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const sessionFile = path.join(tempHome, ".codex", "sessions", "2026", "codex-exec-session.jsonl");

    await writeJsonLines(sessionFile, [
      {
        type: "session_meta",
        timestamp: "2026-01-01T00:00:00.000Z",
        payload: { id: "codex-exec-session", cwd: worktreePath, timestamp: "2026-01-01T00:00:00.000Z", source: "exec" },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:01:00.000Z",
        payload: { type: "message", role: "developer", content: [{ type: "input_text", text: "<permissions instructions>\nSandbox metadata\n</permissions instructions>" }] },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:02:00.000Z",
        payload: { type: "message", role: "user", content: [{ type: "input_text", text: `<environment_context>\n  <cwd>${worktreePath}</cwd>\n</environment_context>` }] },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:03:00.000Z",
        payload: { type: "message", role: "user", content: [{ type: "input_text", text: "Run the headless smoke check." }] },
      },
      {
        type: "event_msg",
        timestamp: "2026-01-01T00:04:00.000Z",
        payload: { type: "agent_message", message: "KANVIBE_CODEX_HEADLESS_OK", phase: "final_answer" },
      },
    ]);

    const sessions = await readCodexSessions({ worktreePath, repoPath: worktreePath, query: "KANVIBE_CODEX_HEADLESS_OK" });
    expect(sessions).toMatchObject({ provider: "codex", available: true, sessionCount: 1 });
    expect(sessions.sessions[0]).toMatchObject({
      id: "codex-exec-session",
      provider: "codex",
      title: "Run the headless smoke check.",
      firstUserPrompt: "Run the headless smoke check.",
      messageCount: 2,
      sourceRef: sessionFile,
    });

    const detail = await readCodexSessionDetail(
      { worktreePath, repoPath: worktreePath },
      "codex-exec-session",
      sessionFile,
      null,
      20,
    );

    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["assistant", "KANVIBE_CODEX_HEADLESS_OK"],
      ["user", "Run the headless smoke check."],
    ]);
  });

  it("uses the latest Codex user or assistant message as the session activity time", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const sessionFile = path.join(tempHome, ".codex", "sessions", "2026", "codex-activity.jsonl");

    await writeJsonLines(sessionFile, [
      {
        type: "session_meta",
        timestamp: "2026-01-01T00:00:00.000Z",
        payload: { id: "codex-activity", cwd: worktreePath, timestamp: "2026-01-01T00:00:00.000Z" },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:01:00.000Z",
        payload: { type: "message", role: "user", content: [{ type: "input_text", text: "Update the chat." }] },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:02:00.000Z",
        payload: { type: "message", role: "assistant", content: [{ type: "output_text", text: "Chat updated." }] },
      },
      {
        type: "response_item",
        timestamp: "2026-01-01T00:03:00.000Z",
        payload: { type: "reasoning", text: "Internal follow-up bookkeeping." },
      },
    ]);

    const sessions = await readCodexSessions({ worktreePath, repoPath: worktreePath });

    expect(sessions.sessions[0]?.updatedAt).toBe("2026-01-01T00:02:00.000Z");
  });

  it("reads Gemini chat recordings from ~/.gemini/tmp/<project>/chats and classifies responses, thoughts, and tool calls", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const chatFile = path.join(tempHome, ".gemini", "tmp", "task", "chats", "session-2026-01-01T00-00-gemini.json");
    await writeJson(path.join(tempHome, ".gemini", "projects.json"), {
      projects: {
        [path.resolve(worktreePath)]: "task",
      },
    });
    await writeJson(chatFile, {
      sessionId: "gemini-session",
      projectHash: "unused",
      startTime: "2026-01-01T00:00:00.000Z",
      lastUpdated: "2026-01-01T00:04:00.000Z",
      kind: "main",
      messages: [
        { id: "user-1", timestamp: "2026-01-01T00:01:00.000Z", type: "user", content: [{ text: "make a plan" }] },
        {
          id: "gemini-1",
          timestamp: "2026-01-01T00:02:00.000Z",
          type: "gemini",
          model: "gemini-2.5-pro",
          content: [{ text: "Here is the plan." }],
          thoughts: [{ subject: "analysis", description: "Compare history formats." }],
          toolCalls: [{ name: "read_file", args: { path: "x" }, result: "file contents" }],
        },
        { id: "info-1", timestamp: "2026-01-01T00:03:00.000Z", type: "info", content: "checkpoint saved" },
      ],
    });

    const sessions = await readGeminiSessions({ worktreePath, repoPath: worktreePath });
    expect(sessions.sessions).toHaveLength(1);
    expect(sessions.sessions[0]).toMatchObject({
      id: "gemini-session",
      provider: "gemini",
      title: "make a plan",
      updatedAt: "2026-01-01T00:02:00.000Z",
      sourceRef: chatFile,
    });

    const detail = await readGeminiSessionDetail(
      { worktreePath, repoPath: worktreePath },
      "gemini-session",
      chatFile,
      null,
      20,
    );

    expect(detail?.messages.map((message) => message.role)).toEqual([
      "system",
      "tool",
      "reasoning",
      "assistant",
      "user",
    ]);
    expect(detail?.messages.find((message) => message.role === "tool")?.fullText).toContain("read_file");
  });

  it("discovers Claude Code sessions by parsing JSONL cwd when the encoded project directory lookup misses", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const sessionFile = path.join(tempHome, ".claude", "projects", "legacy-or-aliased-project-dir", "claude-session.jsonl");

    await writeJsonLines(sessionFile, [
      {
        type: "user",
        sessionId: "claude-aliased-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:01:00.000Z",
        message: { role: "user", content: [{ type: "text", text: "load aliased claude history" }] },
      },
      {
        type: "assistant",
        sessionId: "claude-aliased-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:02:00.000Z",
        message: { role: "assistant", content: [{ type: "text", text: "aliased history loaded" }] },
      },
    ]);

    const sessions = await readClaudeSessions({ worktreePath, repoPath: worktreePath });

    expect(sessions.sessions).toHaveLength(1);
    expect(sessions.sessions[0]).toMatchObject({
      id: "claude-aliased-session",
      provider: "claude",
      matchedPath: worktreePath,
      firstUserPrompt: "load aliased claude history",
      sourceRef: sessionFile,
    });
  });

  it("reads the latest Claude conversation time beyond the summary head", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const sessionFile = path.join(
      tempHome,
      ".claude",
      "projects",
      claudeProjectDirectoryName(worktreePath),
      "long-claude-session.jsonl",
    );
    const toolEvents = Array.from({ length: 65 }, (_, index) => ({
      type: "progress",
      sessionId: "long-claude-session",
      cwd: worktreePath,
      timestamp: `2026-01-01T00:00:${String(index + 2).padStart(2, "0")}.000Z`,
      message: { role: "user", content: [{ type: "tool_result", content: `tool ${index} ${"x".repeat(4096)}` }] },
    }));

    await writeJsonLines(sessionFile, [
      {
        type: "user",
        sessionId: "long-claude-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:00:00.000Z",
        message: { role: "user", content: [{ type: "text", text: "Start the long task." }] },
      },
      {
        type: "assistant",
        sessionId: "long-claude-session",
        cwd: worktreePath,
        timestamp: "2026-01-01T00:10:00.000Z",
        message: { role: "assistant", content: [{ type: "text", text: "The long task is complete." }] },
      },
      ...toolEvents,
    ]);

    const sessions = await readClaudeSessions({ worktreePath, repoPath: worktreePath });

    expect(sessions.sessions[0]?.updatedAt).toBe("2026-01-01T00:10:00.000Z");
  });

  it("uses Gemini CLI .project_root metadata when projects.json does not map the chat directory", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const projectDir = path.join(tempHome, ".gemini", "tmp", "project-with-root-file");
    const chatFile = path.join(projectDir, "chats", "session-2026-01-01T00-20-gemini.json");

    await mkdir(projectDir, { recursive: true });
    await writeFile(path.join(projectDir, ".project_root"), worktreePath, "utf-8");
    await writeJson(chatFile, {
      sessionId: "gemini-root-file-session",
      startTime: "2026-01-01T00:20:00.000Z",
      lastUpdated: "2026-01-01T00:22:00.000Z",
      messages: [
        { id: "user-1", timestamp: "2026-01-01T00:21:00.000Z", type: "user", content: [{ text: "load project root gemini history" }] },
        { id: "assistant-1", timestamp: "2026-01-01T00:22:00.000Z", type: "gemini", content: [{ text: "project root history loaded" }] },
      ],
    });

    const sessions = await readGeminiSessions({ worktreePath, repoPath: worktreePath });

    expect(sessions.sessions).toHaveLength(1);
    expect(sessions.sessions[0]).toMatchObject({
      id: "gemini-root-file-session",
      provider: "gemini",
      matchedPath: worktreePath,
      firstUserPrompt: "load project root gemini history",
      sourceRef: chatFile,
    });

    const detail = await readGeminiSessionDetail({ worktreePath, repoPath: worktreePath }, "gemini-root-file-session", chatFile);
    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["assistant", "project root history loaded"],
      ["user", "load project root gemini history"],
    ]);
  });

  it("falls back to Python sqlite for local OpenCode history when native better-sqlite3 cannot open the DB", async () => {
    const worktreePath = path.join(tempHome, "repo__worktrees", "task");
    const dbPath = path.join(tempHome, ".local", "share", "opencode", "opencode.db");
    const seedScript = `
import json
import sqlite3
import sys

db_path, directory = sys.argv[1], sys.argv[2]
conn = sqlite3.connect(db_path)
cur = conn.cursor()
cur.executescript("""
CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT NOT NULL, title TEXT, time_created INTEGER, time_updated INTEGER);
CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER NOT NULL);
""")
cur.execute("INSERT INTO session VALUES (?, ?, ?, ?, ?)", ("local-open", directory, "Local OpenCode", 1700000000000, 1700000100000))
for message_id, role, text, timestamp in [
    ("user-message", "user", "load local opencode history", 1700000000000),
    ("assistant-message", "assistant", "local opencode history loaded", 1700000010000),
]:
    cur.execute("INSERT INTO message VALUES (?, ?, ?)", (message_id, "local-open", json.dumps({"role": role})))
    cur.execute("INSERT INTO part VALUES (?, ?, ?, ?, ?)", (f"{message_id}-part", "local-open", message_id, json.dumps({"type": "text", "text": text}), timestamp))
conn.commit()
conn.close()
`;
    await mkdir(path.dirname(dbPath), { recursive: true });
    await new Promise<void>((resolve, reject) => {
      execFile("python3", ["-c", seedScript, dbPath, worktreePath], (error) => error ? reject(error) : resolve());
    });

    vi.resetModules();
    vi.doMock("@/lib/sqliteConnectionPool", () => ({
      getSqliteConnection: vi.fn(() => {
        throw new Error("native sqlite unavailable");
      }),
      querySqlite: vi.fn(),
    }));
    const { readOpenCodeSessionDetail, readOpenCodeSessions } = await import("@/lib/aiSessions/readOpenCodeSessions");

    const sessions = await readOpenCodeSessions({ worktreePath, repoPath: worktreePath });
    expect(sessions).toMatchObject({ provider: "opencode", available: true, sessionCount: 1 });
    expect(sessions.sessions[0]).toMatchObject({
      id: "local-open",
      provider: "opencode",
      matchedPath: worktreePath,
      title: "Local OpenCode",
      firstUserPrompt: "load local opencode history",
      updatedAt: new Date(1700000010000).toISOString(),
      sourceRef: "local-open",
    });

    const detail = await readOpenCodeSessionDetail({ worktreePath, repoPath: worktreePath }, "local-open", "local-open");
    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["assistant", "local opencode history loaded"],
      ["user", "load local opencode history"],
    ]);
  });

  it("reads remote Claude Code worktree sessions and detail over SSH", async () => {
    const worktreePath = "/remote/repo__worktrees/task";
    const repoPath = "/remote/repo";
    const sessionFile = `/remote/home/.claude/projects/${claudeProjectDirectoryName(worktreePath)}/remote-claude.jsonl`;

    vi.resetModules();
    const { execCalls } = setupRemoteFileSystem({
      [sessionFile]: jsonLines([
        {
          type: "user",
          sessionId: "remote-claude",
          cwd: worktreePath,
          timestamp: "2026-01-01T00:01:00.000Z",
          message: { role: "user", content: [{ type: "text", text: "load repo claude history" }] },
        },
        {
          type: "assistant",
          sessionId: "remote-claude",
          cwd: worktreePath,
          timestamp: "2026-01-01T00:02:00.000Z",
          message: { role: "assistant", content: [{ type: "text", text: "Claude history loaded remotely." }] },
        },
      ]),
    });
    const { readClaudeSessionDetail, readClaudeSessions } = await import("@/lib/aiSessions/readClaudeSessions");

    const sessions = await readClaudeSessions({ worktreePath, repoPath, sshHost: "remote-host" });
    expect(sessions).toMatchObject({ provider: "claude", available: true, sessionCount: 1 });
    expect(sessions.sessions[0]).toMatchObject({
      id: "remote-claude",
      provider: "claude",
      matchedPath: worktreePath,
      matchScope: "worktree",
      firstUserPrompt: "load repo claude history",
      sourceRef: sessionFile,
    });

    const detail = await readClaudeSessionDetail(
      { worktreePath, repoPath, sshHost: "remote-host" },
      "remote-claude",
      sessionFile,
      null,
      20,
    );

    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["assistant", "Claude history loaded remotely."],
      ["user", "load repo claude history"],
    ]);
    expect(execCalls.length).toBeGreaterThan(0);
    expect(execCalls.every((call) => call.sshHost === "remote-host")).toBe(true);
  });

  it("reads remote Codex worktree sessions and detail over SSH", async () => {
    const worktreePath = "/remote/repo__worktrees/task";
    const repoPath = "/remote/repo";
    const sessionFile = "/remote/home/.codex/sessions/2026/remote-codex.jsonl";

    vi.resetModules();
    const { execCalls } = setupRemoteFileSystem({
      [sessionFile]: jsonLines([
        {
          type: "session_meta",
          timestamp: "2026-01-01T00:00:00.000Z",
          payload: { id: "remote-codex", cwd: worktreePath, timestamp: "2026-01-01T00:00:00.000Z" },
        },
        {
          type: "response_item",
          timestamp: "2026-01-01T00:01:00.000Z",
          payload: { type: "message", role: "user", content: [{ type: "input_text", text: "load repo codex history" }] },
        },
        {
          type: "response_item",
          timestamp: "2026-01-01T00:02:00.000Z",
          payload: { type: "message", role: "assistant", content: [{ type: "output_text", text: "Codex history loaded remotely." }] },
        },
      ]),
    });
    const { readCodexSessionDetail, readCodexSessions } = await import("@/lib/aiSessions/readCodexSessions");

    const sessions = await readCodexSessions({ worktreePath, repoPath, sshHost: "remote-host" });
    expect(sessions).toMatchObject({ provider: "codex", available: true, sessionCount: 1 });
    expect(sessions.sessions[0]).toMatchObject({
      id: "remote-codex",
      provider: "codex",
      matchedPath: worktreePath,
      matchScope: "worktree",
      firstUserPrompt: "load repo codex history",
      sourceRef: sessionFile,
    });

    const detail = await readCodexSessionDetail(
      { worktreePath, repoPath, sshHost: "remote-host" },
      "remote-codex",
      sessionFile,
      null,
      20,
    );

    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["assistant", "Codex history loaded remotely."],
      ["user", "load repo codex history"],
    ]);
    expect(execCalls.length).toBeGreaterThan(0);
    expect(execCalls.every((call) => call.sshHost === "remote-host")).toBe(true);
  });

  it("reads remote Gemini CLI worktree sessions and detail over SSH", async () => {
    const worktreePath = "/remote/repo__worktrees/task";
    const repoPath = "/remote/repo";
    const projectsFile = "/remote/home/.gemini/projects.json";
    const chatFile = "/remote/home/.gemini/tmp/remote-repo/chats/remote-gemini.json";

    vi.resetModules();
    const { execCalls } = setupRemoteFileSystem({
      [projectsFile]: JSON.stringify({ projects: { [worktreePath]: "remote-repo" } }),
      [chatFile]: JSON.stringify({
        sessionId: "remote-gemini",
        startTime: "2026-01-01T00:00:00.000Z",
        lastUpdated: "2026-01-01T00:02:00.000Z",
        messages: [
          { id: "user-1", timestamp: "2026-01-01T00:01:00.000Z", type: "user", content: [{ text: "load repo gemini history" }] },
          { id: "assistant-1", timestamp: "2026-01-01T00:02:00.000Z", type: "gemini", content: [{ text: "Gemini history loaded remotely." }] },
        ],
      }),
    });
    const { readGeminiSessionDetail, readGeminiSessions } = await import("@/lib/aiSessions/readGeminiSessions");

    const sessions = await readGeminiSessions({ worktreePath, repoPath, sshHost: "remote-host" });
    expect(sessions).toMatchObject({ provider: "gemini", available: true, sessionCount: 1 });
    expect(sessions.sessions[0]).toMatchObject({
      id: "remote-gemini",
      provider: "gemini",
      matchedPath: worktreePath,
      matchScope: "worktree",
      firstUserPrompt: "load repo gemini history",
      sourceRef: chatFile,
    });

    const detail = await readGeminiSessionDetail(
      { worktreePath, repoPath, sshHost: "remote-host" },
      "remote-gemini",
      chatFile,
      null,
      20,
    );

    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["assistant", "Gemini history loaded remotely."],
      ["user", "load repo gemini history"],
    ]);
    expect(execCalls.length).toBeGreaterThan(0);
    expect(execCalls.every((call) => call.sshHost === "remote-host")).toBe(true);
  });

  it("reads remote OpenCode sessions and detail over SSH", async () => {
    const worktreePath = "/remote/repo__worktrees/task";
    const repoPath = "/remote/repo";
    const execCalls: Array<{ command: string; sshHost?: string | null; options?: { maxBufferBytes?: number } }> = [];
    let remoteSqliteQueryCount = 0;

    vi.resetModules();
    vi.doMock("@/lib/gitOperations", () => ({
      execGit: vi.fn(async (command: string, sshHost?: string | null, options?: { maxBufferBytes?: number }) => {
        execCalls.push({ command, sshHost, options });
        if (command === "printf '%s' \"$HOME\"") {
          return "/remote/home";
        }
        if (command.includes("test -e") && command.includes("opencode.db")) {
          return "1";
        }
        if (command.includes("python3 -c")) {
          remoteSqliteQueryCount += 1;
          if (remoteSqliteQueryCount === 1) {
            return JSON.stringify([
              {
                id: "remote-open",
                directory: worktreePath,
                title: "Remote OpenCode",
                time_created: 1_700_000_000_000,
                time_updated: 1_700_000_010_000,
                part_count: 2,
                first_user_part: JSON.stringify({ type: "text", text: "fix remote chat" }),
                matching_part_count: 0,
              },
            ]);
          }

          return JSON.stringify([
            {
              session_id: "remote-open",
              directory: worktreePath,
              title: "Remote OpenCode",
              message_id: "user-message",
              part_data: JSON.stringify({ type: "text", text: "fix remote chat" }),
              time_created: 1_700_000_000_000,
              message_data: JSON.stringify({ role: "user" }),
            },
            {
              session_id: "remote-open",
              directory: worktreePath,
              title: "Remote OpenCode",
              message_id: "assistant-message",
              part_data: JSON.stringify({ type: "text", text: "remote history loaded" }),
              time_created: 1_700_000_010_000,
              message_data: JSON.stringify({ role: "assistant" }),
            },
          ]);
        }
        throw new Error(`unexpected remote command: ${command}`);
      }),
    }));

    const { readOpenCodeSessionDetail, readOpenCodeSessions } = await import("@/lib/aiSessions/readOpenCodeSessions");

    const sessions = await readOpenCodeSessions({ worktreePath, repoPath, sshHost: "remote-host" });
    expect(sessions).toMatchObject({
      provider: "opencode",
      available: true,
      sessionCount: 1,
      reason: null,
    });
    expect(sessions.sessions[0]).toMatchObject({
      id: "remote-open",
      provider: "opencode",
      matchedPath: worktreePath,
      title: "Remote OpenCode",
      firstUserPrompt: "fix remote chat",
      sourceRef: "remote-open",
    });

    const detail = await readOpenCodeSessionDetail(
      { worktreePath, repoPath, sshHost: "remote-host" },
      "remote-open",
      "remote-open",
      null,
      20,
    );

    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([
      ["assistant", "remote history loaded"],
      ["user", "fix remote chat"],
    ]);
    expect(execCalls.every((call) => call.sshHost === "remote-host")).toBe(true);
    expect(execCalls
      .filter((call) => call.command.includes("python3 -c"))
      .map((call) => call.options?.maxBufferBytes))
      .toEqual([64 * 1024 * 1024, 64 * 1024 * 1024]);
    expect(remoteSqliteQueryCount).toBe(2);
  });

  it("treats OpenCode body search wildcard characters as literal text", async () => {
    // 세션 필터는 directory 값에도 부분 문자열 매칭을 하므로, OS 임시 경로에 우연히 `_`나 `%`가
    // 들어 있으면 모든 행이 걸려 검증이 무의미해진다. macOS의 /var/folders/... 경로가 실제로
    // 그렇기 때문에 이 테스트만 와일드카드 문자가 없는 고정 경로를 쓴다.
    const worktreePath = "/kanvibe-opencode-fixture/repo/task";
    const rows = [
      {
        id: "literal-percent",
        directory: worktreePath,
        title: "Percent",
        time_created: 1_700_000_000_000,
        time_updated: 1_700_000_000_000,
        part_count: 1,
        first_user_part: JSON.stringify({ type: "text", text: "deployment reached 100% complete" }),
      },
      {
        id: "literal-underscore",
        directory: worktreePath,
        title: "Underscore",
        time_created: 1_700_000_001_000,
        time_updated: 1_700_000_001_000,
        part_count: 1,
        first_user_part: JSON.stringify({ type: "text", text: "rename user_name field" }),
      },
      {
        id: "plain-text",
        directory: worktreePath,
        title: "Plain",
        time_created: 1_700_000_002_000,
        time_updated: 1_700_000_002_000,
        part_count: 1,
        first_user_part: JSON.stringify({ type: "text", text: "database migration rollback" }),
      },
    ];
    const queryCalls: Array<{ sql: string; parameters?: Record<string, unknown> }> = [];

    vi.resetModules();
    vi.doMock("@/lib/sqliteConnectionPool", () => ({
      getSqliteConnection: () => ({}),
      querySqlite: (_db: unknown, sql: string, parameters?: Record<string, unknown>) => {
        queryCalls.push({ sql, parameters });
        const query = parameters?.query;
        const matchingIds = query === "\\%"
          ? new Set(["literal-percent"])
          : query === "\\_"
            ? new Set(["literal-underscore"])
            : query === "database migration"
              ? new Set(["plain-text"])
              : new Set<string>();

        return rows.map((row) => ({
          ...row,
          matching_part_count: matchingIds.has(row.id) ? 1 : 0,
        }));
      },
    }));
    const { readOpenCodeSessions } = await import("@/lib/aiSessions/readOpenCodeSessions");

    const percentMatches = await readOpenCodeSessions({ worktreePath, repoPath: worktreePath, query: "%" });
    expect(percentMatches.sessions.map((session) => session.id)).toEqual(["literal-percent"]);
    expect(queryCalls.at(-1)?.parameters).toEqual({ query: "\\%" });

    const underscoreMatches = await readOpenCodeSessions({ worktreePath, repoPath: worktreePath, query: "_" });
    expect(underscoreMatches.sessions.map((session) => session.id)).toEqual(["literal-underscore"]);
    expect(queryCalls.at(-1)?.parameters).toEqual({ query: "\\_" });

    const bodyMatches = await readOpenCodeSessions({ worktreePath, repoPath: worktreePath, query: "database migration" });
    expect(bodyMatches.sessions.map((session) => session.id)).toEqual(["plain-text"]);
    expect(queryCalls.at(-1)?.parameters).toEqual({ query: "database migration" });

    expect(queryCalls.every((call) => call.sql.includes("ESCAPE"))).toBe(true);
  });
});
