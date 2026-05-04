import { describe, expect, it } from "vitest";
import {
  captureShortcutFromEvent,
  formatShortcutForDisplay,
  getShortcutPlatformFromNavigator,
  isBlockedElectronShortcutInput,
  isBlockedShortcutEvent,
  matchElectronShortcutInput,
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

  it("macOS 기본 modifier 입력은 portable Mod shortcut으로 캡처한다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "p",
      metaKey: true,
      shiftKey: true,
    });

    expect(captureShortcutFromEvent(event, "mac")).toBe("Mod+Shift+P");
  });

  it("Linux 기본 modifier 입력은 portable Mod shortcut으로 캡처한다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(captureShortcutFromEvent(event, "linux")).toBe("Mod+Shift+P");
  });

  it("navigator 정보에서 shortcut platform을 판별한다", () => {
    expect(getShortcutPlatformFromNavigator({
      userAgent: "Mozilla/5.0",
      platform: "MacIntel",
    })).toBe("mac");

    expect(getShortcutPlatformFromNavigator({
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      platform: "Linux x86_64",
    })).toBe("linux");
  });

  it("Electron input도 platform별 Mod 조합으로 매칭한다", () => {
    expect(matchElectronShortcutInput({
      type: "keyDown",
      isAutoRepeat: false,
      key: "n",
      meta: true,
      control: false,
      alt: false,
      shift: false,
    }, "Mod+N", "mac")).toBe(true);

    expect(matchElectronShortcutInput({
      type: "keyDown",
      isAutoRepeat: false,
      key: "n",
      meta: true,
      control: false,
      alt: false,
      shift: false,
    }, "Mod+N", "linux")).toBe(false);
  });

  it("Cmd/Ctrl+R은 앱에서 차단할 shortcut으로 판별한다", () => {
    expect(isBlockedShortcutEvent(new KeyboardEvent("keydown", {
      key: "r",
      metaKey: true,
    }), "mac")).toBe(true);

    expect(isBlockedShortcutEvent(new KeyboardEvent("keydown", {
      key: "r",
      ctrlKey: true,
    }), "linux")).toBe(true);

    expect(isBlockedShortcutEvent(new KeyboardEvent("keydown", {
      key: "r",
      ctrlKey: true,
      shiftKey: true,
    }), "linux")).toBe(false);
  });

  it("Electron Cmd/Ctrl+R input도 앱에서 차단할 shortcut으로 판별한다", () => {
    expect(isBlockedElectronShortcutInput({
      type: "keyDown",
      isAutoRepeat: false,
      key: "r",
      control: true,
    }, "linux")).toBe(true);
  });

  it("Cmd/Ctrl+R은 사용자 지정 shortcut으로 캡처하지 않는다", () => {
    expect(captureShortcutFromEvent(new KeyboardEvent("keydown", {
      key: "r",
      ctrlKey: true,
    }), "linux")).toBeNull();
  });

  it("modifier만 누른 경우는 캡처하지 않는다", () => {
    const event = new KeyboardEvent("keydown", {
      key: "Meta",
      metaKey: true,
    });

    expect(captureShortcutFromEvent(event)).toBeNull();
  });
});
