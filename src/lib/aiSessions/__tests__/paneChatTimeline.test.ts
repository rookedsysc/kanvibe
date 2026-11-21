import { describe, expect, it } from "vitest";
import { advancePaneChatTimeline, isPaneChatCursorCurrent, readPaneChatTimelineSeq, type PaneChatTimeline } from "../paneChatTimeline";
import type { AggregatedAiMessage } from "../types";

const message = (text: string, role: AggregatedAiMessage["role"] = "assistant"): AggregatedAiMessage => ({ role, timestamp: null, text, fullText: text, isTruncated: false });
const start = (texts: string[], sessionId = "session-a"): PaneChatTimeline => advancePaneChatTimeline(null, sessionId, texts.map((text) => message(text))).timeline;
const seqs = (rows: { seq: number }[]) => rows.map((row) => row.seq);

describe("pane 대화 기록의 순번", () => {
  it("처음 읽은 기록은 1번부터 차례로 매기고 교체로 보지 않는다", () => {
    const advance = advancePaneChatTimeline(null, "session-a", [message("a"), message("b")]);
    expect(seqs(advance.changedRows)).toEqual([1, 2]);
    expect(advance.isReplaced).toBe(false);
    expect(readPaneChatTimelineSeq(advance.timeline)).toBe(2);
  });

  it("뒤에 붙은 줄에만 이어지는 순번을 준다", () => {
    const timeline = start(["a", "b", "c"]);
    const advance = advancePaneChatTimeline(timeline, "session-a", ["a", "b", "c", "d", "e"].map((text) => message(text)));
    expect(advance.changedRows).toEqual([{ seq: 4, message: message("d") }, { seq: 5, message: message("e") }]);
    expect(advance.timeline.epoch).toBe(timeline.epoch);
    expect(advance.isReplaced).toBe(false);
  });

  it("마지막 줄의 글이 길어지면 같은 순번으로 다시 내보낸다", () => {
    const timeline = start(["a", "답변을"]);
    const advance = advancePaneChatTimeline(timeline, "session-a", [message("a"), message("답변을 이어 씁니다")]);
    expect(advance.changedRows).toEqual([{ seq: 2, message: message("답변을 이어 씁니다") }]);
    expect(advance.timeline.epoch).toBe(timeline.epoch);
  });

  it("조회 창 밖으로 밀려난 오래된 줄은 어긋남으로 보지 않는다", () => {
    const timeline = start(["a", "b", "c", "d"]);
    const advance = advancePaneChatTimeline(timeline, "session-a", ["c", "d", "e", "f"].map((text) => message(text)));
    expect(seqs(advance.changedRows)).toEqual([5, 6]);
    expect(seqs(advance.timeline.rows)).toEqual([3, 4, 5, 6]);
    expect(advance.isReplaced).toBe(false);
  });

  it("변한 것이 없으면 아무 줄도 내보내지 않는다", () => {
    const timeline = start(["a", "b"]);
    const advance = advancePaneChatTimeline(timeline, "session-a", [message("a"), message("b")]);
    expect(advance.changedRows).toEqual([]);
    expect(advance.timeline).toEqual(timeline);
  });

  it.each([
    ["중간 줄이 달라진 기록", "session-a", ["a", "달라짐", "c"]],
    ["마지막 줄이 다른 글로 바뀐 기록", "session-a", ["a", "b", "전혀 다른 글"]],
    ["비워진 기록", "session-a", []],
    ["다른 세션의 기록", "session-b", ["a", "b", "c"]],
  ])("%s은 새 epoch으로 처음부터 센다", (_, sessionId, texts) => {
    const timeline = start(["a", "b", "c"]);
    const advance = advancePaneChatTimeline(timeline, sessionId, texts.map((text) => message(text)));
    expect(advance.isReplaced).toBe(true);
    expect(advance.timeline.epoch).not.toBe(timeline.epoch);
    expect(seqs(advance.timeline.rows)).toEqual(texts.map((_, index) => index + 1));
  });

  it("같은 문장이 연달아 와도 줄마다 다른 순번을 준다", () => {
    const timeline = start(["네"]);
    const advance = advancePaneChatTimeline(timeline, "session-a", [message("네"), message("네")]);
    expect(advance.changedRows).toEqual([{ seq: 2, message: message("네") }]);
  });

  it("비어 있던 기록에 첫 줄이 생기면 같은 epoch에서 1번을 준다", () => {
    const timeline = start([]);
    const advance = advancePaneChatTimeline(timeline, "session-a", [message("첫 줄", "user")]);
    expect(advance.changedRows).toEqual([{ seq: 1, message: message("첫 줄", "user") }]);
    expect(advance.timeline.epoch).toBe(timeline.epoch);
    expect(advance.isReplaced).toBe(false);
  });

  it.each([
    ["끝과 같은 위치", (timeline: PaneChatTimeline) => ({ epoch: timeline.epoch, seq: 3 }), true],
    ["뒤처진 위치", (timeline: PaneChatTimeline) => ({ epoch: timeline.epoch, seq: 2 }), false],
    ["다른 epoch의 위치", () => ({ epoch: "other", seq: 3 }), false],
    ["위치 없음", () => null, false],
  ])("구독자가 가진 %s를 최신 여부로 판정한다", (_, cursorFor, expected) => {
    const timeline = start(["a", "b", "c"]);
    expect(isPaneChatCursorCurrent(timeline, cursorFor(timeline))).toBe(expected);
  });

  it("기록을 아직 읽지 않은 pane은 어떤 위치도 최신이 아니다", () => {
    expect(isPaneChatCursorCurrent(null, { epoch: "any", seq: 0 })).toBe(false);
  });
});
