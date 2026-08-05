import { describe, expect, it, vi } from "vitest";
import { createTerminalOptions, createTerminalTheme, installMacShiftSelectionPatch } from "@/lib/terminalMouseSelection";

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

  it("완전 불투명한 터미널은 반투명 렌더링을 켜지 않는다", () => {
    // Given
    const fontFamily = "monospace";

    // When
    const options = createTerminalOptions(fontFamily, 1);

    // Then
    expect(options.allowTransparency).toBe(false);
    expect(options.theme?.background).toBe("#0a0a0a");
  });

  it("반투명 터미널은 배경색에 요청된 불투명도를 alpha로 붙인다", () => {
    // Given
    const fontFamily = "monospace";

    // When
    const options = createTerminalOptions(fontFamily, 0.8);

    // Then
    expect(options.allowTransparency).toBe(true);
    expect(options.theme?.background).toBe("#0a0a0acc");
  });

  it("허용 범위를 벗어난 불투명도는 최소값 alpha로 보정한다", () => {
    // Given
    const requestedOpacity = 0;

    // When
    const theme = createTerminalTheme(requestedOpacity);

    // Then
    expect(theme.background).toBe("#0a0a0a4d");
  });
});
