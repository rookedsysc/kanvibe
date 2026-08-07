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
    expect(options).toEqual({ transparent: true, backgroundColor: "#010a0a0a" });
  });

  it("투명 창 배경색 RGB는 alpha가 무시되는 플랫폼에서도 흰 화면이 되지 않도록 어둡게 둔다", () => {
    // Given
    const { createWindowBackgroundOptions } = require("./windowAppearance");

    // When
    const { backgroundColor } = createWindowBackgroundOptions(0.8);

    // Then
    const [red, green, blue] = [3, 5, 7].map((start) => Number.parseInt(backgroundColor.slice(start, start + 2), 16));
    expect(Math.max(red, green, blue)).toBeLessThan(64);
  });

  it("투명 창 배경색은 Electron의 #AARRGGBB 순서로 0이 아닌 최소 alpha를 앞에 둔다", () => {
    // Given
    const { createWindowBackgroundOptions } = require("./windowAppearance");

    // When
    const { backgroundColor } = createWindowBackgroundOptions(0.8);

    // Then
    const alphaChannel = Number.parseInt(backgroundColor.slice(1, 3), 16);
    expect(alphaChannel).toBe(1);
  });

  it("투명도를 읽지 못했으면 창을 투명하게 만들지 않는다", () => {
    // Given
    const { createWindowBackgroundOptions } = require("./windowAppearance");

    // When
    const options = createWindowBackgroundOptions(undefined);

    // Then
    expect(options).toEqual({ backgroundColor: "#ffffff" });
  });

  it("Linux에서 터미널이 반투명하면 ARGB visual 스위치를 켠다", () => {
    // Given
    const { shouldEnableTransparentVisuals } = require("./windowAppearance");

    // When
    const shouldEnable = shouldEnableTransparentVisuals("linux", 0.8);

    // Then
    expect(shouldEnable).toBe(true);
  });

  it("Linux에서도 터미널이 불투명하면 ARGB visual 스위치를 켜지 않는다", () => {
    // Given
    const { shouldEnableTransparentVisuals } = require("./windowAppearance");

    // When
    const shouldEnable = shouldEnableTransparentVisuals("linux", 1);

    // Then
    expect(shouldEnable).toBe(false);
  });

  it("macOS는 창 서버가 알파를 처리하므로 ARGB visual 스위치를 켜지 않는다", () => {
    // Given
    const { shouldEnableTransparentVisuals } = require("./windowAppearance");

    // When
    const shouldEnable = shouldEnableTransparentVisuals("darwin", 0.3);

    // Then
    expect(shouldEnable).toBe(false);
  });

  it("투명도를 읽지 못했으면 ARGB visual 스위치를 켜지 않는다", () => {
    // Given
    const { shouldEnableTransparentVisuals } = require("./windowAppearance");

    // When
    const shouldEnable = shouldEnableTransparentVisuals("linux", undefined);

    // Then
    expect(shouldEnable).toBe(false);
  });
});
