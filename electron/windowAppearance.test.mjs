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
    expect(options).toEqual({ transparent: true, backgroundColor: "#00000000" });
  });

  it("투명도를 읽지 못했으면 창을 투명하게 만들지 않는다", () => {
    // Given
    const { createWindowBackgroundOptions } = require("./windowAppearance");

    // When
    const options = createWindowBackgroundOptions(undefined);

    // Then
    expect(options).toEqual({ backgroundColor: "#ffffff" });
  });
});
