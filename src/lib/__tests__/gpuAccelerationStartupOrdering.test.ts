import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("GPU 가속 비활성화 시작 순서", () => {
  it("app.disableHardwareAcceleration()을 await 뒤가 아니라 모듈 최상위에서 동기적으로 호출한다", () => {
    const source = readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");

    const syncOpacityRead = source.match(
      /registerRuntimeAliases\(\);\nstartupTerminalOpacity = readStartupTerminalOpacitySync\(\);\n\n[\s\S]*?if \(process\.platform !== "linux" && shouldDisableGpuAccelerationForTransparency\(startupTerminalOpacity\)\) \{\n\s+app\.disableHardwareAcceleration\(\);/,
    );

    expect(syncOpacityRead).not.toBeNull();
  });

  it("터미널 투명도를 async 함수 안에서 기다린 뒤에는 disableHardwareAcceleration을 호출하지 않는다", () => {
    const source = readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");

    expect(source).not.toMatch(/await loadStartupTerminalOpacity/);
    expect(source).not.toMatch(/async \(\) => \{[\s\S]*?app\.disableHardwareAcceleration/);
  });
});
