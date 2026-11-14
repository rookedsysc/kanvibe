import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("embedded document navigation", () => {
  it("창 안에 넣은 문서가 스스로 다른 주소로 이동하는 것을 막는다", () => {
    const source = readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");
    const handler = /webContents\.on\("will-frame-navigate", \(event\) => \{\s*if \(!event\.isMainFrame\) event\.preventDefault\(\);\s*\}\);/;

    /** 앱 화면 자체의 이동까지 막으면 창이 첫 화면을 열지 못한다 */
    expect(source).toMatch(handler);
  });
});
