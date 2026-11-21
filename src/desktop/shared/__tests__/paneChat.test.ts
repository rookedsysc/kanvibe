import { describe, expect, it } from "vitest";
import { filterPaneChatSuggestions, type PaneChatSuggestion } from "../paneChat";

const suggestion = (label: string, description = ""): PaneChatSuggestion => ({ id: `skill:${label}`, label, description, insertText: `/${label}` });

describe("filterPaneChatSuggestions", () => {
  it("이름이나 설명에 검색어가 들어 있는 제안을 대소문자 구분 없이 이름순으로 남긴다", () => {
    const suggestions = [suggestion("release", "Ship a build"), suggestion("commit", "Write a REVIEW-ready message"), suggestion("Review", "Check a diff"), suggestion("plan", "Outline work")];
    const result = filterPaneChatSuggestions(suggestions, "review", 20);
    expect(result.items.map((item) => item.label)).toEqual(["commit", "Review"]);
    expect(result.truncated).toBe(false);
  });

  it("검색어가 비어 있으면 전체를 이름순으로 돌려준다", () => {
    const result = filterPaneChatSuggestions([suggestion("b"), suggestion("a")], "", 20);
    expect(result.items.map((item) => item.label)).toEqual(["a", "b"]);
  });

  it("상한과 같은 개수는 잘리지 않았다고 알리고, 하나라도 넘으면 상한까지만 남기고 더 있다고 알린다", () => {
    const suggestions = Array.from({ length: 3 }, (_, index) => suggestion(`skill-${index}`));
    expect(filterPaneChatSuggestions(suggestions, "skill", 3)).toMatchObject({ truncated: false, items: { length: 3 } });
    const bounded = filterPaneChatSuggestions(suggestions, "skill", 2);
    expect(bounded.items.map((item) => item.label)).toEqual(["skill-0", "skill-1"]);
    expect(bounded.truncated).toBe(true);
  });

  it("일치하는 제안이 없으면 빈 목록을 돌려준다", () => {
    expect(filterPaneChatSuggestions([suggestion("plan")], "zzz", 20)).toEqual({ items: [], truncated: false });
  });
});
