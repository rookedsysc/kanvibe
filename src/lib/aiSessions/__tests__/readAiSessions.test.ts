import { mkdir, mkdtemp, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readClaudeSessionDetail } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexSessionDetail } from "@/lib/aiSessions/readCodexSessions";
import { readGeminiSessionDetail, readGeminiSessions } from "@/lib/aiSessions/readGeminiSessions";

let tempHome: string;

async function writeJson(filePath: string, value: unknown) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf-8");
}

async function writeJsonLines(filePath: string, values: unknown[]) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${values.map((value) => JSON.stringify(value)).join("\n")}\n`, "utf-8");
}

function claudeProjectDirectoryName(targetPath: string) {
  return path.resolve(targetPath).replaceAll(path.sep, "-").replaceAll("_", "-");
}

describe("AI session history readers", () => {
  beforeEach(async () => {
    tempHome = await mkdtemp(path.join(tmpdir(), "kanvibe-ai-sessions-"));
    vi.stubEnv("HOME", tempHome);
  });

  afterEach(async () => {
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
});
