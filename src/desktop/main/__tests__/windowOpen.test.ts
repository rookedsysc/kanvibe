// @vitest-environment node
import { describe, expect, it } from "vitest";
import { resolveWindowOpenAction } from "@/desktop/main/windowOpen";

describe("resolveWindowOpenAction", () => {
  it("file 기반 내부 URL은 독립적인 내부 창 열기로 판단한다", () => {
    const result = resolveWindowOpenAction({
      targetUrl: "file:///Applications/Kanvibe.app/Contents/Resources/app.asar/build/renderer/index.html#/ko/task/task-1",
      rendererDevUrl: null,
      openWindows: [],
      getWindowUrl: (windowUrl) => windowUrl,
    });

    expect(result).toEqual({
      type: "open-internal",
      route: "/ko/task/task-1",
      outlivesOpener: true,
    });
  });

  it("이미 열린 동일 route 창이 있으면 새 창 대신 해당 창으로 포커스를 이동시킨다", () => {
    const existingWindow = {
      id: "window-2",
      url: "http://localhost:3000/#/ko/task/task-1",
    };

    const result = resolveWindowOpenAction({
      targetUrl: "http://localhost:3000/#/ko/task/task-1",
      rendererDevUrl: "http://localhost:3000",
      openWindows: [existingWindow],
      getWindowUrl: (windowRecord) => windowRecord.url,
    });

    expect(result).toEqual({
      type: "focus-existing",
      route: "/ko/task/task-1",
      existingWindow,
    });
  });

  it("현재 창만 같은 route면 제외하고 새 내부 창을 연다", () => {
    const sourceWindow = {
      id: "window-1",
      url: "http://localhost:3000/#/ko/task/task-1",
    };

    const result = resolveWindowOpenAction({
      targetUrl: "/#/ko/task/task-1",
      rendererDevUrl: "http://localhost:3000",
      openWindows: [sourceWindow],
      getWindowUrl: (windowRecord) => windowRecord.url,
      excludeWindow: sourceWindow,
    });

    expect(result).toEqual({
      type: "open-internal",
      route: "/ko/task/task-1",
      outlivesOpener: true,
    });
  });

  it("외부 URL은 내부 창 정책 대상이 아니다", () => {
    const result = resolveWindowOpenAction({
      targetUrl: "https://example.com/docs",
      rendererDevUrl: "http://localhost:3000",
      openWindows: [],
      getWindowUrl: (windowUrl) => windowUrl,
    });

    expect(result).toEqual({
      type: "external",
    });
  });
});
