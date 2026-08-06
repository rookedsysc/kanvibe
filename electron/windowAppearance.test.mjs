import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);

describe("Electron window appearance", () => {
  it("터미널이 불투명하면 기존 흰 배경 창을 그대로 만든다", () => {
    // Given
    const { createWindowBackgroundOptions } = require("./windowAppearance");

    // When
    const options = createWindowBackgroundOptions(1);

    // Then
    expect(options).toEqual({ backgroundColor: "#ffffff" });
  });

  it("터미널이 반투명하면 창을 투명하게 만든다", () => {
    // Given
    const { createWindowBackgroundOptions } = require("./windowAppearance");

    // When
    const options = createWindowBackgroundOptions(0.8);

    // Then
    expect(options).toEqual({ transparent: true, backgroundColor: "#00000001", resizable: false });
  });

  it("투명도를 읽지 못했으면 창을 투명하게 만들지 않는다", () => {
    // Given
    const { createWindowBackgroundOptions } = require("./windowAppearance");

    // When
    const options = createWindowBackgroundOptions(undefined);

    // Then
    expect(options).toEqual({ backgroundColor: "#ffffff" });
  });

  it("터미널이 반투명하면 GPU 가속을 꺼야 한다고 판단한다", () => {
    // Given
    const { shouldDisableGpuAccelerationForTransparency } = require("./windowAppearance");

    // When
    const shouldDisable = shouldDisableGpuAccelerationForTransparency(0.8);

    // Then
    expect(shouldDisable).toBe(true);
  });

  it("터미널이 불투명하면 GPU 가속을 그대로 둔다", () => {
    // Given
    const { shouldDisableGpuAccelerationForTransparency } = require("./windowAppearance");

    // When
    const shouldDisable = shouldDisableGpuAccelerationForTransparency(1);

    // Then
    expect(shouldDisable).toBe(false);
  });

  it("투명도를 읽지 못했으면 GPU 가속을 그대로 둔다", () => {
    // Given
    const { shouldDisableGpuAccelerationForTransparency } = require("./windowAppearance");

    // When
    const shouldDisable = shouldDisableGpuAccelerationForTransparency(undefined);

    // Then
    expect(shouldDisable).toBe(false);
  });
});
