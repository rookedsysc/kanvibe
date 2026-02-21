import { describe, it, expect } from "vitest";
import { computeProjectColor } from "@/lib/projectColor";

const PRESET_COLORS = [
  "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
  "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];

describe("computeProjectColor", () => {
  it("should return a deterministic color for the same project name", () => {
    // Given
    const projectName = "kanvibe";

    // When
    const first = computeProjectColor(projectName);
    const second = computeProjectColor(projectName);

    // Then
    expect(first).toBe(second);
  });

  it("should return a color from the preset palette", () => {
    // Given
    const projectNames = ["kanvibe", "my-app", "test-project", "dashboard"];

    // When & Then
    for (const name of projectNames) {
      expect(PRESET_COLORS).toContain(computeProjectColor(name));
    }
  });

  it("should return different colors for different project names", () => {
    // Given
    const names = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"];

    // When
    const colors = names.map(computeProjectColor);
    const uniqueColors = new Set(colors);

    // Then
    expect(uniqueColors.size).toBeGreaterThan(1);
  });

  it("should handle empty string without error", () => {
    // Given & When
    const result = computeProjectColor("");

    // Then
    expect(PRESET_COLORS).toContain(result);
  });

  it("should handle single character names", () => {
    // Given & When
    const result = computeProjectColor("a");

    // Then
    expect(PRESET_COLORS).toContain(result);
  });
});
