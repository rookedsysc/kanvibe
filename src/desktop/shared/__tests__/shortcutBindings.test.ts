import { describe, expect, it } from "vitest";
import {
  TASK_DETAIL_DOCK_SHORTCUT_INDEXES,
  TERMINAL_TAB_SHORTCUT_INDEXES,
  type ElectronShortcutInput,
  type ShortcutPlatformInput,
} from "@/desktop/shared/keyboardShortcut";
import {
  DEFAULT_SHORTCUT_BINDINGS,
  SHORTCUT_COMMAND_DEFINITIONS,
  collectShortcutOverrides,
  findShortcutCommandConflict,
  findShortcutCommandForElectronInput,
  findShortcutCommandForEvent,
  getTaskDetailDockIndexForCommand,
  getTerminalTabPositionForCommand,
  parseShortcutOverrides,
  resolveShortcutBindings,
  resolveTerminalTabCommand,
} from "@/desktop/shared/shortcutBindings";

function dockIndexForEvent(event: KeyboardEvent, platform: ShortcutPlatformInput) {
  return getTaskDetailDockIndexForCommand(findShortcutCommandForEvent(DEFAULT_SHORTCUT_BINDINGS, event, platform));
}

function tabPositionForEvent(event: KeyboardEvent, platform: ShortcutPlatformInput) {
  return getTerminalTabPositionForCommand(findShortcutCommandForEvent(DEFAULT_SHORTCUT_BINDINGS, event, platform));
}

function terminalTabCommandForInput(
  input: ElectronShortcutInput,
  platform: ShortcutPlatformInput,
  isTaskDetailRoute: boolean,
) {
  return resolveTerminalTabCommand(
    findShortcutCommandForElectronInput(DEFAULT_SHORTCUT_BINDINGS, input, platform),
    isTaskDetailRoute,
  );
}

function terminalTabCommandForEvent(
  event: KeyboardEvent,
  platform: ShortcutPlatformInput,
  isTaskDetailRoute: boolean,
) {
  return resolveTerminalTabCommand(
    findShortcutCommandForEvent(DEFAULT_SHORTCUT_BINDINGS, event, platform),
    isTaskDetailRoute,
  );
}

const macInput = (key: string, modifiers: { meta?: boolean; alt?: boolean; shift?: boolean } = {}) => ({
  type: "keyDown",
  isAutoRepeat: false,
  key,
  meta: modifiers.meta ?? false,
  control: false,
  alt: modifiers.alt ?? false,
  shift: modifiers.shift ?? false,
});

describe("단축키 명령 판정", () => {
  it("탭 번호 단축키는 일치한 번호를 반환하고 dock 단축키와 충돌하지 않는다", () => {
    for (const shortcutIndex of TERMINAL_TAB_SHORTCUT_INDEXES) {
      const macEvent = new KeyboardEvent("keydown", {
        key: String(shortcutIndex),
        metaKey: true,
        altKey: true,
      });

      expect(tabPositionForEvent(macEvent, "mac")).toBe(shortcutIndex);
      expect(dockIndexForEvent(macEvent, "mac")).toBeNull();
    }
  });

  it("dock 번호 입력은 탭 번호 단축키로 매칭하지 않는다", () => {
    const dockEvent = new KeyboardEvent("keydown", { key: "1", ctrlKey: true });

    expect(dockIndexForEvent(dockEvent, "linux")).toBe(1);
    expect(tabPositionForEvent(dockEvent, "linux")).toBeNull();
  });

  it("macOS dock 입력은 Option이 없어 탭 번호 단축키와 갈린다", () => {
    const dockEvent = new KeyboardEvent("keydown", { key: "1", metaKey: true });

    expect(dockIndexForEvent(dockEvent, "mac")).toBe(1);
    expect(tabPositionForEvent(dockEvent, "mac")).toBeNull();
  });

  it("Electron input도 탭 번호 단축키로 매칭한다", () => {
    expect(getTerminalTabPositionForCommand(findShortcutCommandForElectronInput(DEFAULT_SHORTCUT_BINDINGS, {
      type: "keyDown",
      key: "3",
      control: true,
      alt: true,
    }, "linux"))).toBe(3);
  });

  it("상세 dock shortcut matcher는 일치한 dock 번호를 반환한다", () => {
    expect(dockIndexForEvent(new KeyboardEvent("keydown", { key: "4", metaKey: true }), "mac")).toBe(4);
    expect(dockIndexForEvent(new KeyboardEvent("keydown", { key: "4", ctrlKey: true }), "linux")).toBe(4);
    expect(dockIndexForEvent(new KeyboardEvent("keydown", { key: "4", altKey: true }), "linux")).toBeNull();
  });
});

describe("터미널 탭 단축키 명령 해석", () => {
  it("태스크 상세에서 새 탭·이전·다음 명령을 만든다", () => {
    expect(terminalTabCommandForInput(macInput("t", { meta: true }), "mac", true))
      .toEqual({ type: "new-tab" });
    expect(terminalTabCommandForInput(macInput("{", { meta: true, shift: true }), "mac", true))
      .toEqual({ type: "previous-tab" });
    expect(terminalTabCommandForInput(macInput("}", { meta: true, shift: true }), "mac", true))
      .toEqual({ type: "next-tab" });
  });

  it("숫자 단축키는 이동할 탭 위치를 담는다", () => {
    expect(terminalTabCommandForInput(macInput("3", { meta: true, alt: true }), "mac", true))
      .toEqual({ type: "go-to-tab", position: 3 });
  });

  it("탭 닫기는 태스크 상세에서만 탭을 닫고 밖에서는 창을 닫는다", () => {
    expect(terminalTabCommandForInput(macInput("w", { meta: true }), "mac", true))
      .toEqual({ type: "close-tab" });
    expect(terminalTabCommandForInput(macInput("w", { meta: true }), "mac", false))
      .toEqual({ type: "close-window" });
  });

  it("창 닫기는 어느 화면에서나 동작한다", () => {
    expect(terminalTabCommandForInput(macInput("w", { meta: true, shift: true }), "mac", false))
      .toEqual({ type: "close-window" });
  });

  it("태스크 상세가 아니면 탭 조작 명령을 만들지 않는다", () => {
    expect(terminalTabCommandForInput(macInput("t", { meta: true }), "mac", false)).toBeNull();
    expect(terminalTabCommandForInput(macInput("3", { meta: true, shift: true }), "mac", false)).toBeNull();
  });

  it("페이지 이동 단축키는 탭 명령으로 새지 않는다", () => {
    expect(terminalTabCommandForInput(macInput("[", { meta: true }), "mac", true)).toBeNull();
    expect(terminalTabCommandForInput(macInput("]", { meta: true }), "mac", true)).toBeNull();
  });

  it("Electron 입력과 렌더러 이벤트가 같은 명령을 만든다", () => {
    const linuxEvent = new KeyboardEvent("keydown", { key: "t", ctrlKey: true });
    const linuxInput = { type: "keyDown", key: "t", control: true, meta: false, alt: false, shift: false };

    expect(terminalTabCommandForEvent(linuxEvent, "linux", true)).toEqual({ type: "new-tab" });
    expect(terminalTabCommandForInput(linuxInput, "linux", true)).toEqual({ type: "new-tab" });
  });

  it("렌더러 경로도 태스크 상세 밖에서는 탭 조작을 만들지 않는다", () => {
    expect(terminalTabCommandForEvent(new KeyboardEvent("keydown", { key: "t", ctrlKey: true }), "linux", false))
      .toBeNull();
  });

  it("렌더러 경로에서도 탭 번호와 창 닫기를 해석한다", () => {
    expect(terminalTabCommandForEvent(
      new KeyboardEvent("keydown", { key: "2", ctrlKey: true, altKey: true }),
      "linux",
      true,
    )).toEqual({ type: "go-to-tab", position: 2 });

    expect(terminalTabCommandForEvent(
      new KeyboardEvent("keydown", { key: "w", ctrlKey: true, shiftKey: true }),
      "linux",
      false,
    )).toEqual({ type: "close-window" });
  });
});

describe("AI 사용량 패널 단축키", () => {
  it("Mod+0을 사용량 단축키로 인식한다", () => {
    expect(findShortcutCommandForEvent(
      DEFAULT_SHORTCUT_BINDINGS,
      new KeyboardEvent("keydown", { key: "0", ctrlKey: true }),
      "linux",
    )).toBe("taskDetailUsage");

    expect(findShortcutCommandForEvent(
      DEFAULT_SHORTCUT_BINDINGS,
      new KeyboardEvent("keydown", { key: "0", metaKey: true }),
      "mac",
    )).toBe("taskDetailUsage");
  });

  it("dock 번호와 겹치지 않는다", () => {
    /** dock 번호 배열은 1~9뿐이어야 0이 dock 항목을 집으려 하지 않는다 */
    expect(TASK_DETAIL_DOCK_SHORTCUT_INDEXES).not.toContain(0);
    expect(dockIndexForEvent(new KeyboardEvent("keydown", { key: "0", ctrlKey: true }), "linux")).toBeNull();
  });

  it("터미널 탭 단축키와도 겹치지 않는다", () => {
    expect(findShortcutCommandForEvent(
      DEFAULT_SHORTCUT_BINDINGS,
      new KeyboardEvent("keydown", { key: "0", ctrlKey: true, altKey: true }),
      "linux",
    )).not.toBe("taskDetailUsage");
  });
});

describe("단축키 재배정", () => {
  it("기본 단축키 표에는 중복 조합이 없다", () => {
    const usedShortcuts = new Set<string>();

    for (const definition of SHORTCUT_COMMAND_DEFINITIONS) {
      expect(usedShortcuts.has(definition.defaultShortcut)).toBe(false);
      usedShortcuts.add(definition.defaultShortcut);
    }
  });

  it("재배정한 조합으로 명령을 판정한다", () => {
    const bindings = resolveShortcutBindings({ taskDetailDock4: "Mod+Shift+K" });

    expect(getTaskDetailDockIndexForCommand(findShortcutCommandForEvent(
      bindings,
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true, shiftKey: true }),
      "linux",
    ))).toBe(4);
    expect(dockIndexForEvent(new KeyboardEvent("keydown", { key: "4", ctrlKey: true }), "linux")).toBe(4);
  });

  it("모르는 명령과 문자열이 아닌 값은 재배정에서 버린다", () => {
    expect(parseShortcutOverrides(JSON.stringify({
      taskDetailDock1: "Mod+Shift+K",
      unknownCommand: "Mod+J",
      taskDetailUsage: 7,
    }))).toEqual({ taskDetailDock1: "Mod+Shift+K" });
  });

  it("저장 형식이 깨져 있으면 재배정 없이 기본값을 쓴다", () => {
    expect(parseShortcutOverrides("{not json")).toEqual({});
    expect(resolveShortcutBindings(parseShortcutOverrides(null))).toEqual(DEFAULT_SHORTCUT_BINDINGS);
  });

  it("기본값과 같아진 항목은 저장하지 않는다", () => {
    expect(collectShortcutOverrides({
      ...DEFAULT_SHORTCUT_BINDINGS,
      taskDetailDock1: "Mod+Shift+K",
    })).toEqual({ taskDetailDock1: "Mod+Shift+K" });
  });

  it("이미 다른 명령이 쓰는 조합은 충돌로 알린다", () => {
    expect(findShortcutCommandConflict(DEFAULT_SHORTCUT_BINDINGS, "taskDetailDock1", "Mod+N"))
      .toBe("createTask");
    expect(findShortcutCommandConflict(DEFAULT_SHORTCUT_BINDINGS, "createTask", "Mod+N"))
      .toBeNull();
    expect(findShortcutCommandConflict(DEFAULT_SHORTCUT_BINDINGS, "taskDetailDock1", "Mod+Shift+K"))
      .toBeNull();
  });
});
