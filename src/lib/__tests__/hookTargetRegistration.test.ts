import { describe, expect, it } from "vitest";
import { verifyHookTargetRegistration } from "@/lib/hookTargetRegistration";

const TARGETS_CONTENT = JSON.stringify({
  schemaVersion: 1,
  targets: [
    { url: "http://127.0.0.1:9736", taskId: "task-local" },
    { url: "http://10.0.0.5:9736", taskId: "task-remote" },
  ],
});

describe("hookTargetRegistration", () => {
  it("현재 client url과 taskId가 모두 등록되어야 설치된 것으로 본다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-local", "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: true,
      registeredHookTargetUrl: "http://127.0.0.1:9736",
    });
  });

  it("다른 client가 같은 task를 등록했더라도 내 client url이 없으면 미설치로 본다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-remote", "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: "http://10.0.0.5:9736",
    });
  });

  it("등록되지 않은 task는 미설치로 본다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-unknown", "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: null,
    });
  });

  it("hook 서버 주소를 확정하지 못하면 taskId 등록 여부만으로 판정한다", () => {
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-remote", null)).toEqual({
      hasRegisteredHookTarget: true,
      registeredHookTargetUrl: "http://10.0.0.5:9736",
    });
    expect(verifyHookTargetRegistration(TARGETS_CONTENT, "task-unknown", null)).toEqual({
      hasRegisteredHookTarget: false,
      registeredHookTargetUrl: null,
    });
  });

  it("확인할 task가 없으면 등록 여부를 판정하지 않는다", () => {
    expect(verifyHookTargetRegistration("", undefined, "http://127.0.0.1:9736")).toEqual({
      hasRegisteredHookTarget: true,
      registeredHookTargetUrl: null,
    });
  });
});
