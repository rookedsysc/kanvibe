import { describe, it, expect } from "vitest";
import { computeProjectColor } from "@/lib/projectColor";

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

    // Then - 최소 2가지 이상 다른 색상이 나와야 한다
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
});
