import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";

const PANE_CHAT_MODES_KEY = "pane_chat_modes";
/** 삭제된 작업의 기록이 끝없이 쌓이지 않도록 최근에 바꾼 작업만 남긴다. 수는 pane 채팅 세션 바인딩의 보관 상한(`MAX_BINDINGS`)과 같다 */
const MAX_TASKS_WITH_MODES = 256;

/** pane ID별 채팅 모드. true는 채팅 UI, false는 사용자가 직접 CLI로 되돌린 pane이다 */
type PaneChatModes = Record<string, boolean>;

let writeQueue: Promise<unknown> = Promise.resolve();

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** 저장값이 깨졌거나 형식이 다르면 그 부분만 버리고 읽을 수 있는 기록은 살린다 */
async function readAllPaneChatModes(): Promise<Map<string, PaneChatModes>> {
  const modesByTask = new Map<string, PaneChatModes>();
  const serialized = await getAppSetting(PANE_CHAT_MODES_KEY);
  if (!serialized) return modesByTask;

  let parsed: unknown;
  try {
    parsed = JSON.parse(serialized);
  } catch {
    return modesByTask;
  }
  if (!isPlainObject(parsed)) return modesByTask;

  for (const [taskId, modes] of Object.entries(parsed)) {
    if (!isPlainObject(modes)) continue;
    const validModes = Object.entries(modes).filter((entry): entry is [string, boolean] => typeof entry[1] === "boolean");
    modesByTask.set(taskId, Object.fromEntries(validModes));
  }
  return modesByTask;
}

export async function getPaneChatModes(taskId: string): Promise<PaneChatModes> {
  return (await readAllPaneChatModes()).get(taskId) ?? {};
}

export function setPaneChatMode(taskId: string, paneId: string, isEnabled: boolean): Promise<void> {
  if (typeof taskId !== "string" || !taskId || typeof paneId !== "string" || !paneId || typeof isEnabled !== "boolean") {
    return Promise.reject(new Error("pane 채팅 모드 저장 요청 형식이 올바르지 않습니다."));
  }

  // 모든 작업의 기록이 설정 한 칸을 공유하므로 데스크톱 화면과 모바일 명령이 같은 읽기-쓰기 순서를 지켜야 서로의 변경을 덮지 않는다.
  const write = writeQueue.then(async () => {
    const modesByTask = await readAllPaneChatModes();
    const taskModes = { ...modesByTask.get(taskId), [paneId]: isEnabled };
    // 다시 넣어 가장 최근에 바꾼 작업이 맨 뒤에 오도록 한다.
    modesByTask.delete(taskId);
    modesByTask.set(taskId, taskModes);
    while (modesByTask.size > MAX_TASKS_WITH_MODES) {
      modesByTask.delete(modesByTask.keys().next().value!);
    }
    await setAppSetting(PANE_CHAT_MODES_KEY, JSON.stringify(Object.fromEntries(modesByTask)));
  });
  // 실패한 호출은 호출자에게 돌려주되 뒤따르는 저장까지 막지 않는다.
  writeQueue = write.catch(() => {});
  return write;
}
