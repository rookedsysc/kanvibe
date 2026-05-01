import { describe, expect, it } from "vitest";
import {
  captureShortcutFromEvent,
  formatShortcutForDisplay,
  matchShortcutEvent,
} from "@/desktop/renderer/utils/keyboardShortcut";

describe("keyboardShortcut", () => {
  it("Mod 단축키는 macOS에서 Cmd로 표시한다", () => {
    expect(formatShortcutForDisplay("Mod+Shift+o", true)).toBe("Cmd+Shift+O");
  });

  it("Mod 단축키는 비 macOS에서 Ctrl로 매칭한다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "o",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(matchShortcutEvent(event, "Mod+Shift+O", false)).toBe(true);
  });

  it("Mod 단축키는 macOS에서 Cmd+Shift+O로 매칭한다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "o",
      metaKey: true,
      shiftKey: true,
    });

    expect(matchShortcutEvent(event, "Mod+Shift+O", true)).toBe(true);
  });

  it("추가 modifier가 있으면 단축키가 일치하지 않는다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "o",
      metaKey: true,
      ctrlKey: true,
      shiftKey: true,
    });

    expect(matchShortcutEvent(event, "Mod+Shift+O", true)).toBe(false);
  });

  it("키 입력으로 explicit shortcut 문자열을 캡처한다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(captureShortcutFromEvent(event)).toBe("Ctrl+Shift+P");
  });

  it("modifier만 누른 경우는 캡처하지 않는다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "Meta",
      metaKey: true,
    });

    expect(captureShortcutFromEvent(event)).toBeNull();
  });
});
