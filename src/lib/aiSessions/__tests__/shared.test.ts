import { describe, expect, it } from "vitest";
import { sortMessagesDescending } from "@/lib/aiSessions/shared";

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
