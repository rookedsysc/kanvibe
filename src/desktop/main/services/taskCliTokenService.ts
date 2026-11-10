import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";

/**
 * task CLI 토큰을 서명하는 이 client 고유의 비밀값.
 * 토큰은 이 값과 taskId로 다시 계산할 수 있으므로 토큰 목록을 따로 저장하지 않는다.
 */
const TASK_CLI_SECRET_SETTING_KEY = "task_cli_secret";

let taskCliSecretPromise: Promise<string> | null = null;

/** 처음 쓰는 순간 비밀값을 만들고, 동시에 들어온 호출이 서로 다른 값을 저장하지 않도록 같은 약속을 공유한다 */
function loadTaskCliSecret(): Promise<string> {
  taskCliSecretPromise ??= (async () => {
    const storedSecret = await getAppSetting(TASK_CLI_SECRET_SETTING_KEY);
    if (storedSecret) {
      return storedSecret;
    }

    const createdSecret = randomBytes(32).toString("hex");
    await setAppSetting(TASK_CLI_SECRET_SETTING_KEY, createdSecret);
    return createdSecret;
  })().catch((error) => {
    taskCliSecretPromise = null;
    throw error;
  });

  return taskCliSecretPromise;
}

/**
 * task 하나에만 통하는 CLI 토큰을 만든다.
 * worktree의 `.kanvibe/targets.json`을 읽을 수 있는 사용자가 그 task가 속한 프로젝트를 다룰 수 있게 하는 근거다.
 */
export async function buildTaskCliToken(taskId: string): Promise<string> {
  return createHmac("sha256", await loadTaskCliSecret()).update(taskId).digest("hex");
}

export async function isValidTaskCliToken(taskId: string, submittedToken: string): Promise<boolean> {
  const expectedToken = Buffer.from(await buildTaskCliToken(taskId), "utf-8");
  const receivedToken = Buffer.from(submittedToken, "utf-8");

  return expectedToken.length === receivedToken.length && timingSafeEqual(expectedToken, receivedToken);
}
