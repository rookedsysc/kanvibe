import { describe, expect, it } from "vitest";
import {
  SHORTCUTS,
  TASK_DETAIL_DOCK_SHORTCUT_INDEXES,
  TERMINAL_TAB_SHORTCUT_INDEXES,
  captureShortcutFromEvent,
  createTaskDetailDockShortcut,
  createTerminalTabShortcut,
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

  it("페이지 이동 단축키는 플랫폼별 조합으로 표시한다", () => {
    expect(formatShortcutForDisplay(SHORTCUTS.pageBack, "mac")).toBe("Cmd+[");
    expect(formatShortcutForDisplay(SHORTCUTS.pageForward, "mac")).toBe("Cmd+]");
    expect(formatShortcutForDisplay(SHORTCUTS.pageBack, "linux")).toBe("Ctrl+[");
    expect(formatShortcutForDisplay(SHORTCUTS.pageForward, "linux")).toBe("Ctrl+]");
  });

  it("페이지 이동 단축키는 플랫폼별 Mod 조합으로 매칭한다", () => {
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "[",
      metaKey: true,
    }), SHORTCUTS.pageBack, "mac")).toBe(true);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "]",
      metaKey: true,
    }), SHORTCUTS.pageForward, "mac")).toBe(true);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "[",
      ctrlKey: true,
    }), SHORTCUTS.pageBack, "linux")).toBe(true);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "]",
      ctrlKey: true,
    }), SHORTCUTS.pageForward, "linux")).toBe(true);
  });

  it("Linux 페이지 이동 단축키는 Alt 조합으로 매칭하지 않는다", () => {
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "[",
      altKey: true,
    }), SHORTCUTS.pageBack, "linux")).toBe(false);
  });

  it("Shift가 섞이면 페이지 이동이 아니라 탭 이동으로 매칭한다", () => {
    const shiftedBracket = new KeyboardEvent("keydown", {
      key: "{",
      metaKey: true,
      shiftKey: true,
    });

    expect(matchShortcutEvent(shiftedBracket, SHORTCUTS.pageBack, "mac")).toBe(false);
    expect(matchShortcutEvent(shiftedBracket, SHORTCUTS.terminalTabPrevious, "mac")).toBe(true);
  });

  it("브라우저가 보고하는 중괄호를 대괄호 단축키로 인식한다", () => {
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "}",
      ctrlKey: true,
      shiftKey: true,
    }), SHORTCUTS.terminalTabNext, "linux")).toBe(true);
  });

  it("탭 조작 단축키는 플랫폼별 Mod 조합으로 표시한다", () => {
    expect(formatShortcutForDisplay(SHORTCUTS.terminalTabNew, "mac")).toBe("Cmd+T");
    expect(formatShortcutForDisplay(SHORTCUTS.terminalTabClose, "mac")).toBe("Cmd+W");
    expect(formatShortcutForDisplay(SHORTCUTS.terminalWindowClose, "mac")).toBe("Cmd+Shift+W");
    expect(formatShortcutForDisplay(SHORTCUTS.terminalTabNew, "linux")).toBe("Ctrl+T");
    expect(formatShortcutForDisplay(SHORTCUTS.terminalWindowClose, "linux")).toBe("Ctrl+Shift+W");
  });

  it("탭 닫기와 윈도우 닫기는 Shift 유무로 갈린다", () => {
    const closeTabEvent = new KeyboardEvent("keydown", { key: "w", ctrlKey: true });
    const closeWindowEvent = new KeyboardEvent("keydown", { key: "w", ctrlKey: true, shiftKey: true });

    expect(matchShortcutEvent(closeTabEvent, SHORTCUTS.terminalTabClose, "linux")).toBe(true);
    expect(matchShortcutEvent(closeTabEvent, SHORTCUTS.terminalWindowClose, "linux")).toBe(false);
    expect(matchShortcutEvent(closeWindowEvent, SHORTCUTS.terminalWindowClose, "linux")).toBe(true);
    expect(matchShortcutEvent(closeWindowEvent, SHORTCUTS.terminalTabClose, "linux")).toBe(false);
  });

  /** macOS는 Cmd+Shift+3~5를 스크린샷으로 가져가 앱에 넘기지 않으므로 탭 단축키가 Shift를 쓰면 죽는다 */
  it("탭 번호 단축키는 macOS 스크린샷 조합인 Cmd+Shift+숫자를 쓰지 않는다", () => {
    for (const shortcutIndex of TERMINAL_TAB_SHORTCUT_INDEXES) {
      expect(formatShortcutForDisplay(createTerminalTabShortcut(shortcutIndex), "mac"))
        .toBe(`Cmd+Option+${shortcutIndex}`);
      expect(formatShortcutForDisplay(createTerminalTabShortcut(shortcutIndex), "linux"))
        .toBe(`Ctrl+Alt+${shortcutIndex}`);
      expect(matchShortcutEvent(new KeyboardEvent("keydown", {
        key: String(shortcutIndex),
        metaKey: true,
        shiftKey: true,
      }), createTerminalTabShortcut(shortcutIndex), "mac")).toBe(false);
    }
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

  it("Electron 페이지 이동 input도 macOS Cmd와 Linux Ctrl로 매칭한다", () => {
    expect(matchElectronShortcutInput({
      type: "keyDown",
      isAutoRepeat: false,
      key: "[",
      meta: true,
      alt: false,
      control: false,
      shift: false,
    }, SHORTCUTS.pageBack, "mac")).toBe(true);

    expect(matchElectronShortcutInput({
      type: "keyDown",
      isAutoRepeat: false,
      key: "]",
      meta: true,
      alt: false,
      control: false,
      shift: false,
    }, SHORTCUTS.pageForward, "mac")).toBe(true);

    expect(matchElectronShortcutInput({
      type: "keyDown",
      isAutoRepeat: false,
      key: "[",
      alt: false,
      meta: false,
      control: true,
      shift: false,
    }, SHORTCUTS.pageBack, "linux")).toBe(true);

    expect(matchElectronShortcutInput({
      type: "keyDown",
      isAutoRepeat: false,
      key: "]",
      alt: false,
      meta: false,
      control: true,
      shift: false,
    }, SHORTCUTS.pageForward, "linux")).toBe(true);
  });

  it("상세 dock shortcut은 macOS Cmd+숫자와 Linux Ctrl+숫자로 표시하고 매칭한다", () => {
    expect(TASK_DETAIL_DOCK_SHORTCUT_INDEXES).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(formatShortcutForDisplay(createTaskDetailDockShortcut(1), "mac")).toBe("Cmd+1");
    expect(formatShortcutForDisplay(createTaskDetailDockShortcut(1), "linux")).toBe("Ctrl+1");

    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      metaKey: true,
    }), createTaskDetailDockShortcut(1), "mac")).toBe(true);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      ctrlKey: true,
    }), createTaskDetailDockShortcut(1), "linux")).toBe(true);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      metaKey: true,
      altKey: true,
    }), createTaskDetailDockShortcut(1), "mac")).toBe(false);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      ctrlKey: true,
      shiftKey: true,
    }), createTaskDetailDockShortcut(1), "linux")).toBe(false);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      altKey: true,
    }), createTaskDetailDockShortcut(1), "linux")).toBe(false);
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
