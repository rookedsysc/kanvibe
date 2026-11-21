import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, expect, it } from "vitest";
import { getAiSessionDetail } from "../aggregateAiSessions";

let fixture: string;
const context = { worktreePath: "/qa/recorded-text-worktree", repoPath: "/qa/recorded-text-worktree" };

beforeEach(async () => { fixture = await mkdtemp(path.join(tmpdir(), "kanvibe-claude-recorded-text-")); });
afterEach(async () => { await rm(fixture, { recursive: true, force: true }); });

/** Claude Code 2.1.326이 실제로 남긴 모양의 사용자 줄들을 대화 기록으로 읽는다. 최신 메시지가 먼저 온다 */
async function readRecordedUserTexts(contents: string[]) {
  const source = path.join(fixture, "session.jsonl");
  const events = contents.map((content, index) => ({ type: "user", sessionId: "recorded-text", cwd: context.worktreePath, timestamp: `2026-11-17T01:00:0${index}Z`, message: { role: "user", content } }));
  await writeFile(source, events.map((event) => JSON.stringify(event)).join("\n"));
  return (await getAiSessionDetail(context, "claude", "recorded-text", source))?.messages.map((message) => message.fullText);
}

it("Claude가 접어 보여 준 붙여넣기는 감싼 표시를 걷어 사용자가 보낸 글 그대로 읽는다", async () => {
  const texts = await readRecordedUserTexts([
    "\n\n<pasted_content id=\"71e5\">\nline 1\nline 2\nline 3\nline 4\n</pasted_content id=\"71e5\">\n",
    "Compare these:\n\n<pasted_content id=\"0a1b\">\nfirst\nsecond\n</pasted_content id=\"0a1b\">\n",
  ]);

  expect(texts).toEqual(["Compare these:\n\nfirst\nsecond", "line 1\nline 2\nline 3\nline 4"]);
});

it("Claude가 명령만 따로 남긴 셸 명령은 사용자가 친 `!` 명령으로 읽는다", async () => {
  expect(await readRecordedUserTexts(["<bash-input>echo hi-from-chat</bash-input>"])).toEqual(["!echo hi-from-chat"]);
});

it("여닫는 표시의 id가 서로 다른 글은 붙여넣기 표시로 보지 않고 그대로 둔다", async () => {
  const literal = "<pasted_content id=\"1\">\ntext\n</pasted_content id=\"2\">";

  expect(await readRecordedUserTexts([literal])).toEqual([literal]);
});
