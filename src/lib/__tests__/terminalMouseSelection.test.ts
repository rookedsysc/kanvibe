import { describe, expect, it, vi } from "vitest";
import { createTerminalOptions, installMacShiftSelectionPatch } from "@/lib/terminalMouseSelection";

describe("terminalMouseSelection", () => {
  it("macOS에서 shift 왼쪽 클릭을 xterm 강제 선택으로 판정한다", () => {
    // Given
    const originalShouldForceSelection = vi.fn((event: MouseEvent) => event.altKey);
    const originalHandleMouseDown = vi.fn();
    const selectionService = {
      shouldForceSelection: originalShouldForceSelection,
      handleMouseDown: originalHandleMouseDown,
    };
    const dispose = installMacShiftSelectionPatch({
      _core: {
        _selectionService: selectionService,
      },
    }, true);
    const event = new MouseEvent("mousedown", {
      button: 0,
      shiftKey: true,
    });

    // When
    const result = selectionService.shouldForceSelection(event);
    dispose();

    // Then
    expect(result).toBe(true);
    expect(originalShouldForceSelection).not.toHaveBeenCalled();
  });

  it("macOS가 아니면 shift 클릭 패치를 적용하지 않는다", () => {
    // Given
    const originalShouldForceSelection = vi.fn((event: MouseEvent) => event.altKey);
    const originalHandleMouseDown = vi.fn();
    const selectionService = {
      shouldForceSelection: originalShouldForceSelection,
      handleMouseDown: originalHandleMouseDown,
    };
    const dispose = installMacShiftSelectionPatch({ _core: { _selectionService: selectionService } }, false);
    const event = new MouseEvent("mousedown", {
      button: 0,
      shiftKey: true,
    });

    // When
    const result = selectionService.shouldForceSelection(event);
    dispose();

    // Then
    expect(result).toBe(false);
  });

  it("macOS에서 handleMouseDown이 shift 클릭을 일반 선택 경로로 정규화한다", () => {
    // Given
    const originalHandleMouseDown = vi.fn();
    const selectionService = {
      shouldForceSelection: vi.fn((event: MouseEvent) => event.altKey),
      handleMouseDown: originalHandleMouseDown,
    };
    const dispose = installMacShiftSelectionPatch({ _core: { _selectionService: selectionService } }, true);

    // When
    selectionService.handleMouseDown(new MouseEvent("mousedown", {
      button: 0,
      buttons: 1,
      shiftKey: true,
      clientX: 10,
      clientY: 20,
      detail: 1,
    }));
    dispose();

    // Then
    const normalizedEvent = originalHandleMouseDown.mock.calls[0]?.[0];
    expect(normalizedEvent).toBeDefined();
    expect(normalizedEvent?.altKey).toBe(true);
    expect(normalizedEvent?.shiftKey).toBe(false);
    expect(normalizedEvent?.clientX).toBe(10);
    expect(normalizedEvent?.clientY).toBe(20);
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
