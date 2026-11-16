import { appendFile, mkdtemp, rm, utimes, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { readCodexSessionDetail } from "@/lib/aiSessions/readCodexSessions";

const SESSION_ID = "codex-reuse";
let directory: string;
let worktreePath: string;
let rolloutFile: string;
let nextModifiedAt: number;

const timestamp = (minute: number) => `2026-01-01T00:${String(minute).padStart(2, "0")}:00.000Z`;
const meta = (sessionId = SESSION_ID) => `${JSON.stringify({ type: "session_meta", timestamp: timestamp(0), payload: { id: sessionId, cwd: worktreePath } })}\n`;
function message(minute: number, role: "user" | "assistant", text: string) {
  const content = [{ type: role === "user" ? "input_text" : "output_text", text }];
  return `${JSON.stringify({ type: "response_item", timestamp: timestamp(minute), payload: { type: "message", role, content } })}\n`;
}
/** 같은 틱 안의 수정도 파일 캐시가 알아채도록 수정 시각을 매번 1초씩 밀어 준다 */
async function markModified() {
  nextModifiedAt += 1;
  await utimes(rolloutFile, nextModifiedAt, nextModifiedAt);
}
const readDetail = (cursor: string | null = null, limit = 20) => readCodexSessionDetail({ worktreePath, repoPath: worktreePath }, SESSION_ID, rolloutFile, cursor, limit);
const texts = (detail: Awaited<ReturnType<typeof readDetail>>) => detail?.messages.map((entry) => entry.fullText);

beforeEach(async () => {
  directory = await mkdtemp(path.join(tmpdir(), "kv-codex-detail-"));
  worktreePath = path.join(directory, "repo");
  rolloutFile = path.join(directory, `${SESSION_ID}.jsonl`);
  nextModifiedAt = 1_800_000_000;
});
afterEach(async () => { await rm(directory, { recursive: true, force: true }); });

describe("readCodexSessionDetail가 읽은 기록을 이어 쓰는 방식", () => {
  it("바뀌지 않은 기록을 다시 읽으면 같은 내용을 돌려주고 이미 만든 메시지를 다시 만들지 않는다", async () => {
    await writeFile(rolloutFile, meta() + message(1, "user", "첫 질문") + message(2, "assistant", "첫 답"));
    await markModified();
    const first = await readDetail();
    const second = await readDetail();
    expect(texts(second)).toEqual(["첫 답", "첫 질문"]);
    expect(second).toEqual(first);
    expect(second?.messages[0]).toBe(first?.messages[0]);
  });

  it("기록 뒤에 줄이 붙으면 새 메시지를 최신순 맨 앞에 싣고 앞선 메시지는 다시 만들지 않는다", async () => {
    await writeFile(rolloutFile, meta() + message(1, "user", "첫 질문") + message(2, "assistant", "첫 답"));
    await markModified();
    const first = await readDetail();
    await appendFile(rolloutFile, message(3, "user", "다음 질문") + message(4, "assistant", "다음 답"));
    await markModified();
    const second = await readDetail();
    expect(texts(second)).toEqual(["다음 답", "다음 질문", "첫 답", "첫 질문"]);
    expect(second?.messages[2]).toBe(first?.messages[0]);
    expect(second?.title).toBe("첫 질문");
  });

  it("앞에서 이미 실은 것과 같은 메시지가 뒤에 다시 붙어도 한 번만 싣는다", async () => {
    await writeFile(rolloutFile, meta() + message(1, "user", "같은 질문"));
    await markModified();
    await readDetail();
    await appendFile(rolloutFile, message(1, "user", "같은 질문") + message(2, "assistant", "답"));
    await markModified();
    expect(texts(await readDetail())).toEqual(["답", "같은 질문"]);
  });

  it("다른 대화의 기록이면 줄이 붙은 뒤에도 계속 비어 있다고 알린다", async () => {
    await writeFile(rolloutFile, meta("another-session") + message(1, "user", "남의 질문"));
    await markModified();
    expect(await readDetail()).toBeNull();
    await appendFile(rolloutFile, message(2, "assistant", "남의 답"));
    await markModified();
    expect(await readDetail()).toBeNull();
  });

  it("기록이 더 짧은 내용으로 다시 쓰이면 이전 메시지를 남기지 않는다", async () => {
    await writeFile(rolloutFile, meta() + message(1, "user", "지워질 질문") + message(2, "assistant", "지워질 답"));
    await markModified();
    await readDetail();
    await writeFile(rolloutFile, meta() + message(5, "user", "새 질문"));
    await markModified();
    expect(texts(await readDetail())).toEqual(["새 질문"]);
  });

  it("줄이 붙은 뒤에도 이전 페이지 커서가 같은 순서의 다음 묶음을 가리킨다", async () => {
    await writeFile(rolloutFile, meta() + message(1, "user", "하나") + message(2, "assistant", "둘") + message(3, "user", "셋"));
    await markModified();
    await appendFile(rolloutFile, message(4, "assistant", "넷"));
    await markModified();
    const latest = await readDetail(null, 2);
    expect(texts(latest)).toEqual(["넷", "셋"]);
    const older = await readDetail(latest?.nextCursor, 2);
    expect(texts(older)).toEqual(["둘", "하나"]);
    expect(older?.nextCursor).toBeNull();
  });
});
