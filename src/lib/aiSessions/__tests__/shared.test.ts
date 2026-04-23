import { describe, expect, it } from "vitest";
import { mapWithConcurrency, normalizeText, sortMessagesDescending } from "@/lib/aiSessions/shared";

describe("sortMessagesDescending", () => {
  it("should sort session detail messages from newest to oldest", () => {
    const result = sortMessagesDescending([
      { role: "user", timestamp: "2026-03-13T09:00:00.000Z", text: "old", fullText: "old", isTruncated: false },
      { role: "assistant", timestamp: "2026-03-13T09:02:00.000Z", text: "new", fullText: "new", isTruncated: false },
      { role: "tool", timestamp: "2026-03-13T09:01:00.000Z", text: "mid", fullText: "mid", isTruncated: false },
    ]);

    expect(result.map((message) => message.text)).toEqual(["new", "mid", "old"]);
  });
});

describe("normalizeText", () => {
  it("should remove pasted placeholders and transport noise", () => {
    const result = normalizeText(`Fix hook state\n[Pasted ~33 lines]\n[remote-ssh] command failed {\nsshHost: 'roky-home'\nerror: 'server exited unexpectedly'\n    at createSessionWithoutWorktree (/tmp/worktree.js:1:1)\nmain branch tmux session failed`);

    expect(result).toBe("Fix hook state main branch tmux session failed");
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
