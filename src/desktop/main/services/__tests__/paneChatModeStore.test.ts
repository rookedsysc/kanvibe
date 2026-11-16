import { beforeEach, describe, expect, it, vi } from "vitest";

const PANE_CHAT_MODES_KEY = "pane_chat_modes";
const MAX_TASKS_WITH_MODES = 256;

const settings = vi.hoisted(() => ({ values: new Map<string, string>() }));

/** 읽기와 쓰기 사이에 다른 호출이 끼어들 틈을 만들어, 순서 보장이 없으면 마지막 쓰기가 앞선 쓰기를 덮도록 한다 */
const yieldToOtherCalls = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getAppSetting: async (key: string) => {
    await yieldToOtherCalls();
    return settings.values.get(key) ?? null;
  },
  setAppSetting: async (key: string, value: string) => {
    await yieldToOtherCalls();
    settings.values.set(key, value);
  },
}));

const { getPaneChatModes, setPaneChatMode } = await import("../paneChatModeStore");

beforeEach(() => {
  settings.values.clear();
});

describe("pane 채팅 모드 저장소", () => {
  it("채팅으로 저장한 pane을 작업 단위로 조회한다", async () => {
    await setPaneChatMode("task-1", "%1", true);

    expect(await getPaneChatModes("task-1")).toEqual({ "%1": true });
  });

  it("채팅으로 저장한 pane을 터미널로 되돌리면 직접 닫은 선택으로 false를 기억한다", async () => {
    await setPaneChatMode("task-1", "%1", true);
    await setPaneChatMode("task-1", "%1", false);

    expect(await getPaneChatModes("task-1")).toEqual({ "%1": false });
  });

  it("다른 작업의 기록은 섞이지 않는다", async () => {
    await setPaneChatMode("task-a", "%1", true);

    expect(await getPaneChatModes("task-a")).toEqual({ "%1": true });
    expect(await getPaneChatModes("task-b")).toEqual({});
  });

  it("서로 다른 pane을 동시에 저장해도 둘 다 남는다", async () => {
    await Promise.all([
      setPaneChatMode("task-1", "%1", true),
      setPaneChatMode("task-1", "%2", false),
    ]);

    expect(await getPaneChatModes("task-1")).toEqual({ "%1": true, "%2": false });
  });

  it("저장값이 깨진 JSON이면 빈 기록으로 읽고 이후 저장은 정상 동작한다", async () => {
    settings.values.set(PANE_CHAT_MODES_KEY, "{not json");

    expect(await getPaneChatModes("task-1")).toEqual({});
    await setPaneChatMode("task-1", "%1", true);
    expect(await getPaneChatModes("task-1")).toEqual({ "%1": true });
  });

  it("작업이 상한을 넘으면 가장 오래 전에 바꾼 작업의 기록만 사라진다", async () => {
    for (let index = 1; index <= MAX_TASKS_WITH_MODES; index++) {
      await setPaneChatMode(`task-${index}`, "%1", true);
    }
    // task-1을 다시 바꾸면 가장 오래된 작업은 task-2가 된다
    await setPaneChatMode("task-1", "%2", true);

    await setPaneChatMode(`task-${MAX_TASKS_WITH_MODES + 1}`, "%1", true);

    expect(await getPaneChatModes("task-2")).toEqual({});
    expect(await getPaneChatModes("task-1")).toEqual({ "%1": true, "%2": true });
    expect(await getPaneChatModes("task-3")).toEqual({ "%1": true });
    expect(await getPaneChatModes(`task-${MAX_TASKS_WITH_MODES + 1}`)).toEqual({ "%1": true });
  });
});
