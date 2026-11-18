import { appendFile, mkdtemp, rm, utimes, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { readClaudeSessionDetail } from "@/lib/aiSessions/readClaudeSessions";

const SESSION_ID = "claude-reuse";
let directory: string;
let worktreePath: string;
let sessionFile: string;
let nextModifiedAt: number;

function entry(minute: number, role: "user" | "assistant", text: string, sessionId = SESSION_ID) {
  return `${JSON.stringify({ type: role, sessionId, cwd: worktreePath, timestamp: `2026-01-01T00:${String(minute).padStart(2, "0")}:00.000Z`, message: { role, content: [{ type: "text", text }] } })}\n`;
}
/** 같은 틱 안의 수정도 파일 캐시가 알아채도록 수정 시각을 매번 1초씩 밀어 준다 */
async function markModified() {
  nextModifiedAt += 1;
  await utimes(sessionFile, nextModifiedAt, nextModifiedAt);
}
const readDetail = (cursor: string | null = null, limit = 20) => readClaudeSessionDetail({ worktreePath, repoPath: worktreePath }, SESSION_ID, sessionFile, cursor, limit);
const texts = (detail: Awaited<ReturnType<typeof readDetail>>) => detail?.messages.map((message) => message.fullText);

beforeEach(async () => {
  directory = await mkdtemp(path.join(tmpdir(), "kv-claude-detail-"));
  worktreePath = path.join(directory, "repo");
  sessionFile = path.join(directory, `${SESSION_ID}.jsonl`);
  nextModifiedAt = 1_800_000_000;
});
afterEach(async () => { await rm(directory, { recursive: true, force: true }); });

describe("readClaudeSessionDetail가 읽은 기록을 이어 쓰는 방식", () => {
  it("바뀌지 않은 기록을 다시 읽으면 같은 내용을 돌려주고 이미 만든 메시지를 다시 만들지 않는다", async () => {
    await writeFile(sessionFile, entry(1, "user", "첫 질문") + entry(2, "assistant", "첫 답"));
    await markModified();
    const first = await readDetail();
    const second = await readDetail();
    expect(texts(second)).toEqual(["첫 답", "첫 질문"]);
    expect(second).toEqual(first);
    expect(second?.messages[0]).toBe(first?.messages[0]);
    expect(second?.messages[1]).toBe(first?.messages[1]);
  });

  it("기록 뒤에 줄이 붙으면 새 메시지를 최신순 맨 앞에 싣고 앞선 메시지는 다시 만들지 않는다", async () => {
    await writeFile(sessionFile, entry(1, "user", "첫 질문") + entry(2, "assistant", "첫 답"));
    await markModified();
    const first = await readDetail();
    await appendFile(sessionFile, entry(3, "user", "다음 질문") + entry(4, "assistant", "다음 답"));
    await markModified();
    const second = await readDetail();
    expect(texts(second)).toEqual(["다음 답", "다음 질문", "첫 답", "첫 질문"]);
    expect(second?.messages[2]).toBe(first?.messages[0]);
    expect(second?.title).toBe("첫 질문");
  });

  it("쓰는 중인 마지막 줄은 빼고 읽고, 줄이 완성되면 그 메시지를 싣는다", async () => {
    const unfinished = entry(2, "assistant", "쓰는 중인 답");
    await writeFile(sessionFile, entry(1, "user", "첫 질문") + unfinished.slice(0, 40));
    await markModified();
    expect(texts(await readDetail())).toEqual(["첫 질문"]);
    await appendFile(sessionFile, unfinished.slice(40));
    await markModified();
    expect(texts(await readDetail())).toEqual(["쓰는 중인 답", "첫 질문"]);
  });

  it("기록이 더 짧은 내용으로 다시 쓰이면 이전 메시지를 남기지 않는다", async () => {
    await writeFile(sessionFile, entry(1, "user", "지워질 질문") + entry(2, "assistant", "지워질 답"));
    await markModified();
    await readDetail();
    await writeFile(sessionFile, entry(5, "user", "새 질문"));
    await markModified();
    const detail = await readDetail();
    expect(texts(detail)).toEqual(["새 질문"]);
    expect(detail?.title).toBe("새 질문");
  });

  it("줄이 붙은 뒤에도 이전 페이지 커서가 같은 순서의 다음 묶음을 가리킨다", async () => {
    await writeFile(sessionFile, entry(1, "user", "하나") + entry(2, "assistant", "둘") + entry(3, "user", "셋"));
    await markModified();
    const latest = await readDetail(null, 2);
    expect(texts(latest)).toEqual(["셋", "둘"]);
    expect(texts(await readDetail(latest?.nextCursor, 2))).toEqual(["하나"]);
    await appendFile(sessionFile, entry(4, "assistant", "넷"));
    await markModified();
    const afterAppend = await readDetail(null, 2);
    expect(texts(afterAppend)).toEqual(["넷", "셋"]);
    const older = await readDetail(afterAppend?.nextCursor, 2);
    expect(texts(older)).toEqual(["둘", "하나"]);
    expect(older?.nextCursor).toBeNull();
  });

  it("같은 파일에 섞인 다른 대화의 줄은 붙은 뒤에도 싣지 않는다", async () => {
    await writeFile(sessionFile, entry(1, "user", "내 질문") + entry(2, "user", "남의 질문", "other-session"));
    await markModified();
    await readDetail();
    await appendFile(sessionFile, entry(3, "assistant", "남의 답", "other-session") + entry(4, "assistant", "내 답"));
    await markModified();
    expect(texts(await readDetail())).toEqual(["내 답", "내 질문"]);
  });
});

describe("readClaudeSessionDetail가 대화가 아닌 기록을 다루는 방식", () => {
  const record = (fields: Record<string, unknown>) => `${JSON.stringify({ sessionId: SESSION_ID, ...fields })}\n`;

  it("대화 파일에 함께 남는 API 요청 로그는 메시지로 싣지 않는다", async () => {
    const requestCopy = [{ type: "text", text: "<system-reminder>저장소 지침</system-reminder>" }, { type: "text", text: "질문" }];
    await writeFile(sessionFile, entry(1, "user", "질문")
      + record({ type: "api-request-blob", hash: "request-user", message: { role: "user", content: requestCopy } })
      + record({ type: "api-request-blob", hash: "request-system", message: { role: "system", content: [{ type: "text", text: "hook 출력" }] } })
      + entry(2, "assistant", "답"));
    await markModified();
    expect(texts(await readDetail())).toEqual(["답", "질문"]);
  });

  it("사용자가 친 글이 아니라고 Claude가 표시한 줄은 지침 역할로 싣는다", async () => {
    const skillBody = "Base directory for this skill: /skills/example";
    await writeFile(sessionFile, entry(1, "user", "질문")
      + record({ type: "user", isMeta: true, cwd: worktreePath, timestamp: "2026-01-01T00:02:00.000Z", message: { role: "user", content: skillBody } }));
    await markModified();
    const detail = await readDetail();
    expect(detail?.messages.map((message) => [message.role, message.fullText])).toEqual([["system", skillBody], ["user", "질문"]]);
  });
});
