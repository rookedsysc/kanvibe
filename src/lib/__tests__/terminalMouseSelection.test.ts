import { describe, expect, it } from "vitest";
import { createTerminalOptions, promoteMacShiftClickSelection, registerTerminalMouseSelectionBridge } from "@/lib/terminalMouseSelection";

describe("terminalMouseSelection", () => {
  it("macOS에서 shift 왼쪽 클릭을 xterm 강제 선택 이벤트로 승격한다", () => {
    // Given
    const event = new MouseEvent("mousedown", {
      button: 0,
      shiftKey: true,
    });

    // When
    const promotedEvent = promoteMacShiftClickSelection(event, true);

    // Then
    expect(promotedEvent).not.toBeNull();
    expect(promotedEvent?.altKey).toBe(true);
    expect(promotedEvent?.shiftKey).toBe(false);
  });

  it("macOS가 아니면 shift 클릭 modifier를 건드리지 않는다", () => {
    // Given
    const event = new MouseEvent("mousedown", {
      button: 0,
      shiftKey: true,
    });

    // When
    const promotedEvent = promoteMacShiftClickSelection(event, false);

    // Then
    expect(promotedEvent).toBeNull();
  });

  it("컨테이너 브리지가 capture 단계에서 shift 클릭을 alt 클릭으로 재디스패치한다", () => {
    // Given
    const container = document.createElement("div");
    const target = document.createElement("div");
    container.append(target);
    const dispose = registerTerminalMouseSelectionBridge(container, true);
    const receivedModifiers: Array<{ altKey: boolean; shiftKey: boolean }> = [];
    target.addEventListener("mousedown", (event) => {
      receivedModifiers.push({
        altKey: event.altKey,
        shiftKey: event.shiftKey,
      });
    });

    // When
    target.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      cancelable: true,
      button: 0,
      shiftKey: true,
    }));
    dispose();

    // Then
    expect(receivedModifiers).toEqual([{ altKey: true, shiftKey: false }]);
  });

  it("터미널 옵션에 macOS 강제 선택 옵션을 포함한다", () => {
    // Given
    const fontFamily = "'JetBrainsMono Nerd Font Mono', monospace";

    // When
    const options = createTerminalOptions(fontFamily);

    // Then
    expect(options.fontFamily).toBe(fontFamily);
    expect(options.macOptionClickForcesSelection).toBe(true);
  });
});
