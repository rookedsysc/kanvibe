import { describe, expect, it } from "vitest";
import { extractPlainText, makePreviewMessage, mapWithConcurrency, normalizeText, sortMessagesDescending } from "@/lib/aiSessions/shared";

describe("sortMessagesDescending", () => {
  it("should sort session detail messages from newest to oldest", () => {
    const result = sortMessagesDescending([
      { role: "user", timestamp: "2026-03-13T09:00:00.000Z", text: "old", fullText: "old", isTruncated: false },
      { role: "assistant", timestamp: "2026-03-13T09:02:00.000Z", text: "new", fullText: "new", isTruncated: false },
      { role: "tool", timestamp: "2026-03-13T09:01:00.000Z", text: "mid", fullText: "mid", isTruncated: false },
    ]);

    expect(result.map((message) => message.text)).toEqual(["new", "mid", "old"]);
  });

  it("같은 시각의 사용자 입력과 응답은 클라이언트가 뒤집어도 파일의 추가 순서를 유지한다", () => {
    const messages = [
      { role: "user" as const, timestamp: "2026-11-12T10:00:00.000Z", text: "질문", fullText: "질문", isTruncated: false },
      { role: "assistant" as const, timestamp: "2026-11-12T19:00:00.000+09:00", text: "응답", fullText: "응답", isTruncated: false },
      { role: "user" as const, timestamp: "2026-11-12T10:01:00.000Z", text: "후속 질문", fullText: "후속 질문", isTruncated: false },
    ];
    const result = sortMessagesDescending(messages);
    expect(result.map((message) => message.text)).toEqual(["후속 질문", "응답", "질문"]);
    expect([...result].reverse().map((message) => message.text)).toEqual(["질문", "응답", "후속 질문"]);
    expect(messages.map((message) => message.text)).toEqual(["질문", "응답", "후속 질문"]);
  });

  it("시각이 없는 메시지끼리도 추가 순서를 역순 페이지에 보존한다", () => {
    const messages = [
      { role: "user" as const, timestamp: null, text: "질문", fullText: "질문", isTruncated: false },
      { role: "assistant" as const, timestamp: null, text: "응답", fullText: "응답", isTruncated: false },
    ];
    expect(sortMessagesDescending(messages).map((message) => message.text)).toEqual(["응답", "질문"]);
  });

});

describe("normalizeText", () => {
  it("should remove pasted placeholders and transport noise", () => {
    const result = normalizeText(`Fix hook state\n[Pasted ~33 lines]\n[remote-ssh] command failed {\nsshHost: 'roky-home'\nerror: 'server exited unexpectedly'\n    at createSessionWithoutWorktree (/tmp/worktree.js:1:1)\nmain branch tmux session failed`);

    expect(result).toBe("Fix hook state main branch tmux session failed");
  });
});

describe("AI session message text", () => {
  it("should preserve markdown and mermaid line breaks for detail rendering while keeping a flat preview", () => {
    const source = {
      text: `## Markdown summary

- **Strong** list item
- Safe [link](https://example.com)

\`\`\`ts
const rendered = true;
\`\`\`

\`\`\`mermaid
graph TD
  A[AI history loaded] --> B[Markdown rendered]
  B --> C[Mermaid SVG rendered]
\`\`\``,
    };

    const fullText = extractPlainText(source);
    const previewMessage = makePreviewMessage("assistant", "2026-01-01T00:00:00.000Z", fullText);

    expect(fullText).toContain("## Markdown summary\n");
    expect(fullText).toContain("```mermaid\ngraph TD");
    expect(previewMessage?.text).toContain("## Markdown summary - **Strong** list item");
    expect(previewMessage?.fullText).toBe(fullText);
  });
});

describe("mapWithConcurrency", () => {
  it("should preserve result order while limiting concurrent work", async () => {
    let activeCount = 0;
    let maxActiveCount = 0;

    const result = await mapWithConcurrency([1, 2, 3, 4, 5], 2, async (value, index) => {
      activeCount += 1;
      maxActiveCount = Math.max(maxActiveCount, activeCount);

      await new Promise((resolve) => setTimeout(resolve, (5 - index) * 2));

      activeCount -= 1;
      return value * 10;
    });

    expect(result).toEqual([10, 20, 30, 40, 50]);
    expect(maxActiveCount).toBeLessThanOrEqual(2);
  });
});
