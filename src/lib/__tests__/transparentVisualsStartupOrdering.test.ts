import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

function readElectronMainSource(): string {
  return readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");
}

describe("투명 창 시작 순서", () => {
  it("enable-transparent-visuals 스위치를 await 뒤가 아니라 모듈 최상위에서 동기적으로 붙인다", () => {
    const source = readElectronMainSource();

    const syncSwitchAppend = source.match(
      /registerRuntimeAliases\(\);\nstartupTerminalOpacity = readStartupTerminalOpacitySync\(\);\n\nif \(shouldEnableTransparentVisuals\(process\.platform, startupTerminalOpacity\)\) \{\n\s+app\.commandLine\.appendSwitch\("enable-transparent-visuals"\);/,
    );

    expect(syncSwitchAppend).not.toBeNull();
  });

  it("터미널 투명도를 async 함수 안에서 기다린 뒤에는 시작 스위치를 붙이지 않는다", () => {
    const source = readElectronMainSource();

    expect(source).not.toMatch(/await loadStartupTerminalOpacity/);
    expect(source).not.toMatch(/async \(\) => \{[\s\S]*?appendSwitch\("enable-transparent-visuals"\)/);
  });

  it("터미널 투명도 때문에 GPU 가속을 끄지 않는다", () => {
    const source = readElectronMainSource();

    expect(source).not.toMatch(/shouldDisableGpuAccelerationForTransparency/);
    expect(source).not.toMatch(/appendSwitch\("disable-gpu-compositing"\)/);
    expect(source).not.toMatch(/appendSwitch\("disable-software-rasterizer"\)/);
  });
});
