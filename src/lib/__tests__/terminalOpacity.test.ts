import { describe, expect, it } from "vitest";
import { OPAQUE_TERMINAL_OPACITY, clampTerminalOpacity, isTerminalTransparent } from "@/lib/terminalOpacity";

describe("terminalOpacity", () => {
  it("허용 범위 안의 불투명도는 그대로 둔다", () => {
    // Given
    const requestedOpacity = 0.7;

    // When
    const clampedOpacity = clampTerminalOpacity(requestedOpacity);

    // Then
    expect(clampedOpacity).toBe(0.7);
  });

  it("완전히 투명한 0은 창 합성이 깨지지 않도록 최소값으로 올린다", () => {
    // Given
    const requestedOpacity = 0;

    // When
    const clampedOpacity = clampTerminalOpacity(requestedOpacity);

    // Then
    expect(clampedOpacity).toBe(0.001);
  });

  it("최대값보다 높은 불투명도는 완전 불투명으로 낮춘다", () => {
    // Given
    const requestedOpacity = 9;

    // When
    const clampedOpacity = clampTerminalOpacity(requestedOpacity);

    // Then
    expect(clampedOpacity).toBe(OPAQUE_TERMINAL_OPACITY);
  });

  it("숫자가 아닌 불투명도는 완전 불투명으로 되돌린다", () => {
    // Given
    const requestedOpacity = Number.NaN;

    // When
    const clampedOpacity = clampTerminalOpacity(requestedOpacity);

    // Then
    expect(clampedOpacity).toBe(OPAQUE_TERMINAL_OPACITY);
  });

  it("완전 불투명일 때는 투명 처리 대상이 아니라고 판정한다", () => {
    // Given
    const requestedOpacity = OPAQUE_TERMINAL_OPACITY;

    // When
    const isTransparent = isTerminalTransparent(requestedOpacity);

    // Then
    expect(isTransparent).toBe(false);
  });

  it("완전 불투명보다 낮으면 투명 처리 대상으로 판정한다", () => {
    // Given
    const requestedOpacity = 0.99;

    // When
    const isTransparent = isTerminalTransparent(requestedOpacity);

    // Then
    expect(isTransparent).toBe(true);
  });
});
