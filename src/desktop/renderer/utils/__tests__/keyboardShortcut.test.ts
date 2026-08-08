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
  matchTaskDetailDockShortcutEvent,
  matchTerminalTabShortcutEvent,
  matchTerminalTabShortcutInput,
  resolveTerminalTabShortcutCommand,
  resolveTerminalTabShortcutEvent,
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

  it("탭 번호 단축키는 일치한 번호를 반환하고 dock 단축키와 충돌하지 않는다", () => {
    for (const shortcutIndex of TERMINAL_TAB_SHORTCUT_INDEXES) {
      const macEvent = new KeyboardEvent("keydown", {
        key: String(shortcutIndex),
        metaKey: true,
      });

      expect(matchTerminalTabShortcutEvent(macEvent, "mac")).toBe(shortcutIndex);
      expect(matchTaskDetailDockShortcutEvent(macEvent, "mac")).toBeNull();
    }
  });

  /** macOS는 Cmd+Shift+3~5를 스크린샷으로 가져가 앱에 넘기지 않으므로 탭 단축키가 Shift를 쓰면 죽는다 */
  it("탭 번호 단축키는 macOS 스크린샷 조합인 Cmd+Shift+숫자를 쓰지 않는다", () => {
    for (const shortcutIndex of TERMINAL_TAB_SHORTCUT_INDEXES) {
      expect(formatShortcutForDisplay(createTerminalTabShortcut(shortcutIndex), "mac"))
        .toBe(`Cmd+${shortcutIndex}`);
      expect(matchShortcutEvent(new KeyboardEvent("keydown", {
        key: String(shortcutIndex),
        metaKey: true,
        shiftKey: true,
      }), createTerminalTabShortcut(shortcutIndex), "mac")).toBe(false);
    }
  });

  it("dock 번호 입력은 탭 번호 단축키로 매칭하지 않는다", () => {
    const dockEvent = new KeyboardEvent("keydown", { key: "1", altKey: true });

    expect(matchTaskDetailDockShortcutEvent(dockEvent, "linux")).toBe(1);
    expect(matchTerminalTabShortcutEvent(dockEvent, "linux")).toBeNull();
  });

  it("macOS dock 입력은 Option이 더해져 탭 번호 단축키와 갈린다", () => {
    const dockEvent = new KeyboardEvent("keydown", { key: "1", metaKey: true, altKey: true });

    expect(matchTaskDetailDockShortcutEvent(dockEvent, "mac")).toBe(1);
    expect(matchTerminalTabShortcutEvent(dockEvent, "mac")).toBeNull();
  });

  it("Electron input도 탭 번호 단축키로 매칭한다", () => {
    expect(matchTerminalTabShortcutInput({
      type: "keyDown",
      key: "3",
      control: true,
    }, "linux")).toBe(3);
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

  it("상세 dock shortcut은 macOS Cmd+Option+숫자와 Linux Alt+숫자로 표시하고 매칭한다", () => {
    expect(TASK_DETAIL_DOCK_SHORTCUT_INDEXES).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(formatShortcutForDisplay(createTaskDetailDockShortcut(1), "mac")).toBe("Cmd+Option+1");
    expect(formatShortcutForDisplay(createTaskDetailDockShortcut(1), "linux")).toBe("Alt+1");

    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      metaKey: true,
      altKey: true,
    }), createTaskDetailDockShortcut(1), "mac")).toBe(true);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      altKey: true,
    }), createTaskDetailDockShortcut(1), "linux")).toBe(true);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      metaKey: true,
    }), createTaskDetailDockShortcut(1), "mac")).toBe(false);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      altKey: true,
      shiftKey: true,
    }), createTaskDetailDockShortcut(1), "linux")).toBe(false);
    expect(matchShortcutEvent(new KeyboardEvent("keydown", {
      key: "1",
      ctrlKey: true,
    }), createTaskDetailDockShortcut(1), "linux")).toBe(false);
  });

  it("상세 dock shortcut matcher는 일치한 dock 번호를 반환한다", () => {
    expect(matchTaskDetailDockShortcutEvent(new KeyboardEvent("keydown", {
      key: "4",
      metaKey: true,
      altKey: true,
    }), "mac")).toBe(4);
    expect(matchTaskDetailDockShortcutEvent(new KeyboardEvent("keydown", {
      key: "4",
      altKey: true,
    }), "linux")).toBe(4);
    expect(matchTaskDetailDockShortcutEvent(new KeyboardEvent("keydown", {
      key: "4",
      ctrlKey: true,
    }), "linux")).toBeNull();
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

describe("터미널 탭 단축키 명령 해석", () => {
  const macInput = (key: string, modifiers: { meta?: boolean; shift?: boolean } = {}) => ({
    type: "keyDown",
    isAutoRepeat: false,
    key,
    meta: modifiers.meta ?? false,
    control: false,
    alt: false,
    shift: modifiers.shift ?? false,
  });

  it("태스크 상세에서 새 탭·이전·다음 명령을 만든다", () => {
    expect(resolveTerminalTabShortcutCommand(macInput("t", { meta: true }), "mac", true))
      .toEqual({ type: "new-tab" });
    expect(resolveTerminalTabShortcutCommand(macInput("{", { meta: true, shift: true }), "mac", true))
      .toEqual({ type: "previous-tab" });
    expect(resolveTerminalTabShortcutCommand(macInput("}", { meta: true, shift: true }), "mac", true))
      .toEqual({ type: "next-tab" });
  });

  it("숫자 단축키는 이동할 탭 위치를 담는다", () => {
    expect(resolveTerminalTabShortcutCommand(macInput("3", { meta: true }), "mac", true))
      .toEqual({ type: "go-to-tab", position: 3 });
  });

  it("탭 닫기는 태스크 상세에서만 탭을 닫고 밖에서는 창을 닫는다", () => {
    expect(resolveTerminalTabShortcutCommand(macInput("w", { meta: true }), "mac", true))
      .toEqual({ type: "close-tab" });
    expect(resolveTerminalTabShortcutCommand(macInput("w", { meta: true }), "mac", false))
      .toEqual({ type: "close-window" });
  });

  it("창 닫기는 어느 화면에서나 동작한다", () => {
    expect(resolveTerminalTabShortcutCommand(macInput("w", { meta: true, shift: true }), "mac", false))
      .toEqual({ type: "close-window" });
  });

  it("태스크 상세가 아니면 탭 조작 명령을 만들지 않는다", () => {
    expect(resolveTerminalTabShortcutCommand(macInput("t", { meta: true }), "mac", false)).toBeNull();
    expect(resolveTerminalTabShortcutCommand(macInput("3", { meta: true, shift: true }), "mac", false)).toBeNull();
  });

  it("페이지 이동 단축키는 탭 명령으로 새지 않는다", () => {
    expect(resolveTerminalTabShortcutCommand(macInput("[", { meta: true }), "mac", true)).toBeNull();
    expect(resolveTerminalTabShortcutCommand(macInput("]", { meta: true }), "mac", true)).toBeNull();
  });
});

describe("렌더러 keydown도 같은 탭 명령으로 해석한다", () => {
  it("Electron 입력과 렌더러 이벤트가 같은 명령을 만든다", () => {
    const linuxEvent = new KeyboardEvent("keydown", { key: "t", ctrlKey: true });
    const linuxInput = { type: "keyDown", key: "t", control: true, meta: false, alt: false, shift: false };

    expect(resolveTerminalTabShortcutEvent(linuxEvent, "linux", true)).toEqual({ type: "new-tab" });
    expect(resolveTerminalTabShortcutCommand(linuxInput, "linux", true)).toEqual({ type: "new-tab" });
  });

  it("렌더러 경로도 태스크 상세 밖에서는 탭 조작을 만들지 않는다", () => {
    const newTabEvent = new KeyboardEvent("keydown", { key: "t", ctrlKey: true });

    expect(resolveTerminalTabShortcutEvent(newTabEvent, "linux", false)).toBeNull();
  });

  it("렌더러 경로에서도 탭 번호와 창 닫기를 해석한다", () => {
    expect(resolveTerminalTabShortcutEvent(
      new KeyboardEvent("keydown", { key: "2", ctrlKey: true }),
      "linux",
      true,
    )).toEqual({ type: "go-to-tab", position: 2 });

    expect(resolveTerminalTabShortcutEvent(
      new KeyboardEvent("keydown", { key: "w", ctrlKey: true, shiftKey: true }),
      "linux",
      false,
    )).toEqual({ type: "close-window" });
  });
});
