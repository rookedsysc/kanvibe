/**
 * @vitest-environment node
 */
import { createRequire } from "node:module";
import { beforeEach, describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);
const { runMobileCommand, SUPPORTED_COMMANDS } = require("./mobileCommands");

let bridge;

beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  bridge = {
    deleteMobileTask: vi.fn(async () => ({ failedPeerCount: 0 })),
    closeMobilePane: vi.fn(async () => ({ surfaces: { tabs: [] } })),
    getMobilePaneCloseInfo: vi.fn(async () => ({ processNames: [] })),
  };
});

function command(fields) {
  return JSON.stringify({ type: "command", requestId: "r1", ...fields });
}

describe("소켓 명령 검증", () => {
  it("지원하는 명령 이름을 connected 이벤트와 같은 목록으로 노출한다", () => {
    expect(SUPPORTED_COMMANDS).toEqual(["delete-task", "get-tab-close-info", "close-tab"]);
  });

  it.each(["rename-task", "toString", "constructor", "__proto__", ""])("알 수 없는 명령 %j은 지원하지 않는다는 응답을 requestId와 함께 돌려준다", async (name) => {
    const result = await runMobileCommand(bridge, command({ command: name, taskId: "task-1" }));

    expect(result).toMatchObject({ type: "command-result", requestId: "r1", ok: false, code: "unsupported-command" });
    expect(bridge.deleteMobileTask).not.toHaveBeenCalled();
  });

  it.each([
    ["taskId 누락", { command: "delete-task" }],
    ["taskId가 문자열이 아님", { command: "delete-task", taskId: 7 }],
    ["taskId가 빈 문자열", { command: "delete-task", taskId: "" }],
    ["isForced가 boolean이 아님", { command: "delete-task", taskId: "task-1", isForced: "yes" }],
    ["paneId 누락", { command: "close-tab", taskId: "task-1" }],
    ["paneId가 빈 문자열", { command: "close-tab", taskId: "task-1", paneId: "" }],
    ["조회 명령의 paneId 누락", { command: "get-tab-close-info", taskId: "task-1" }],
  ])("%s이면 잘못된 명령 응답을 돌려주고 브리지를 부르지 않는다", async (_label, fields) => {
    const result = await runMobileCommand(bridge, command(fields));

    expect(result).toMatchObject({ type: "command-result", requestId: "r1", ok: false, code: "invalid-command" });
    expect(bridge.deleteMobileTask).not.toHaveBeenCalled();
    expect(bridge.closeMobilePane).not.toHaveBeenCalled();
    expect(bridge.getMobilePaneCloseInfo).not.toHaveBeenCalled();
  });

  it.each([
    ["requestId 누락", { type: "command", command: "delete-task", taskId: "task-1" }],
    ["requestId가 빈 문자열", { type: "command", requestId: "", command: "delete-task", taskId: "task-1" }],
    ["requestId가 문자열이 아님", { type: "command", requestId: 1, command: "delete-task", taskId: "task-1" }],
    ["requestId가 64자 초과", { type: "command", requestId: "a".repeat(65), command: "delete-task", taskId: "task-1" }],
    ["type이 command가 아님", { type: "ping", requestId: "r1", command: "delete-task", taskId: "task-1" }],
  ])("%s이면 응답할 길이 없으므로 아무것도 하지 않는다", async (_label, message) => {
    expect(await runMobileCommand(bridge, JSON.stringify(message))).toBeNull();
    expect(bridge.deleteMobileTask).not.toHaveBeenCalled();
  });

  it.each(["not json", "null", "[]", "7", '"text"'])("JSON 객체가 아닌 메시지 %s는 무시한다", async (raw) => {
    expect(await runMobileCommand(bridge, raw)).toBeNull();
  });

  it("1024바이트를 넘는 메시지는 파싱하지 않고 무시한다", async () => {
    const oversized = command({ command: "delete-task", taskId: "t".repeat(1024) });

    expect(await runMobileCommand(bridge, Buffer.from(oversized))).toBeNull();
    expect(bridge.deleteMobileTask).not.toHaveBeenCalled();
  });

  it("64자 requestId와 Buffer 입력은 그대로 처리한다", async () => {
    const requestId = "r".repeat(64);
    const raw = Buffer.from(JSON.stringify({ type: "command", requestId, command: "delete-task", taskId: "task-1" }));

    expect(await runMobileCommand(bridge, raw)).toEqual({ type: "command-result", requestId, ok: true, failedPeerCount: 0 });
  });
});
