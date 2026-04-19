import { describe, expect, it } from "vitest";
import { createTerminalOptions, promoteMacShiftClickSelection, registerTerminalMouseSelectionBridge } from "@/lib/terminalMouseSelection";

describe("terminalMouseSelection", () => {
  it("macOS에서 shift 왼쪽 클릭을 xterm 강제 선택으로 승격한다", () => {
    // Given
    const event = new MouseEvent("mousedown", {
      button: 0,
      shiftKey: true,
    });

    // When
    promoteMacShiftClickSelection(event, true);

    // Then
    expect(event.altKey).toBe(true);
  });

  it("macOS가 아니면 shift 클릭 modifier를 건드리지 않는다", () => {
    // Given
    const event = new MouseEvent("mousedown", {
      button: 0,
      shiftKey: true,
    });

    // When
    promoteMacShiftClickSelection(event, false);

    // Then
    expect(event.altKey).toBe(false);
  });

  it("컨테이너 브리지가 capture 단계에서 shift 클릭을 보정한다", () => {
    // Given
    const container = document.createElement("div");
    const dispose = registerTerminalMouseSelectionBridge(container, true);
    const receivedAltKeys: boolean[] = [];
    container.addEventListener("mousedown", (event) => {
      receivedAltKeys.push(event.altKey);
    });

    // When
    container.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      button: 0,
      shiftKey: true,
    }));
    dispose();

    // Then
    expect(receivedAltKeys).toEqual([true]);
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
