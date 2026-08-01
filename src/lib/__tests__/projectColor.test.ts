import { describe, it, expect } from "vitest";
import { computeProjectColor, getReadableTextColor } from "@/lib/projectColor";

describe("computeProjectColor", () => {
  it("should return a valid hex color string", () => {
    // Given
    const projectName = "kanvibe";

    // When
    const color = computeProjectColor(projectName);

    // Then
    expect(color).toMatch(/^#[0-9A-Fa-f]{6}$/);
  });

  it("should return the same color for the same project name (deterministic)", () => {
    // Given
    const projectName = "kanvibe";

    // When
    const color1 = computeProjectColor(projectName);
    const color2 = computeProjectColor(projectName);

    // Then
    expect(color1).toBe(color2);
  });

  it("should return different colors for different project names", () => {
    // Given
    const names = ["kanvibe", "my-app", "backend", "frontend", "docs"];

    // When
    const colors = names.map(computeProjectColor);

    // Then
    const uniqueColors = new Set(colors);
    expect(uniqueColors.size).toBeGreaterThanOrEqual(2);
  });

  it("should return one of the preset colors", () => {
    // Given
    const presetColors = [
      "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
      "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
    ];

    // When
    const color = computeProjectColor("test-project");

    // Then
    expect(presetColors).toContain(color);
  });

  it("should handle empty string without throwing", () => {
    // Given / When
    const color = computeProjectColor("");

    // Then
    expect(color).toMatch(/^#[0-9A-Fa-f]{6}$/);
  });

  it("should handle long project names without throwing", () => {
    // Given
    const longName = "a".repeat(1000);

    // When
    const color = computeProjectColor(longName);

    // Then
    expect(color).toMatch(/^#[0-9A-Fa-f]{6}$/);
  });

  it("should handle single character names", () => {
    // Given
    const presetColors = [
      "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
      "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
    ];

    // When
    const result = computeProjectColor("a");

    // Then
    expect(presetColors).toContain(result);
  });
});

describe("getReadableTextColor", () => {
  it("파스텔 프리셋 색상 위에서는 어두운 글자를 고른다", () => {
    // Given
    const presetColors = [
      "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
      "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
    ];

    // When / Then
    for (const color of presetColors) {
      expect(getReadableTextColor(color)).toBe("#111827");
    }
  });

  it("어두운 색상 위에서는 흰 글자를 고른다", () => {
    // Given / When / Then
    expect(getReadableTextColor("#0064FF")).toBe("#FFFFFF");
    expect(getReadableTextColor("#000000")).toBe("#FFFFFF");
    expect(getReadableTextColor("#202632")).toBe("#FFFFFF");
  });

  it("형식이 잘못된 색상은 어두운 글자로 처리한다", () => {
    // Given / When / Then
    expect(getReadableTextColor("not-a-color")).toBe("#111827");
    expect(getReadableTextColor("#FFF")).toBe("#111827");
    expect(getReadableTextColor("")).toBe("#111827");
  });

  it("앞뒤 공백이 있어도 배경 휘도를 그대로 판정한다", () => {
    // Given / When / Then
    expect(getReadableTextColor("  #000000  ")).toBe("#FFFFFF");
  });
});
