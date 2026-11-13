const path = require("node:path");

/** 사용자 프로젝트에 설치된 hook이 부르는 포트라서, 설치된 앱만 이 포트를 쓴다 */
const PACKAGED_HOOK_SERVER_PORT = 9736;
const DEV_HOOK_SERVER_PORT = 19736;
/** `pnpm start`나 `--qa`로 띄운 수동 QA 실행. 설치된 앱과 같은 포트를 쓰면 둘 중 하나만 hook을 받는다 */
const MANUAL_QA_HOOK_SERVER_PORT = 29736;
/** 자동 QA처럼 실행마다 포트를 직접 정해야 하는 런처가 넘기는 Electron 스위치 */
const HOOK_SERVER_PORT_SWITCH = "kanvibe-hook-port";

function parseRequestedHookServerPort(value) {
  const port = Number(value);
  if (!Number.isInteger(port) || port <= 0 || port > 65535) {
    throw new Error(`Invalid --${HOOK_SERVER_PORT_SWITCH}: ${value}`);
  }
  return port;
}

/** app data 디렉터리를 고르는 기준과 같은 기준으로 포트를 골라, 데이터가 다른 실행끼리는 포트도 겹치지 않게 한다 */
function resolveHookServerPort(app, env = process.env) {
  const requestedPort = app.commandLine.getSwitchValue(HOOK_SERVER_PORT_SWITCH);
  if (requestedPort) {
    return parseRequestedHookServerPort(requestedPort);
  }

  const isManualQa = app.commandLine.hasSwitch("qa");
  if (env.KANVIBE_RENDERER_URL && !isManualQa) return DEV_HOOK_SERVER_PORT;
  if (app.isPackaged && !isManualQa) return PACKAGED_HOOK_SERVER_PORT;
  return MANUAL_QA_HOOK_SERVER_PORT;
}

function getRequestedAppDataDirectory(env = process.env) {
  const value = env.KANVIBE_APP_DATA_DIR;
  if (typeof value !== "string") return null;

  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function applyAppDataDirectoryOverride(app, env = process.env) {
  const requestedAppDataDir = getRequestedAppDataDirectory(env);
  if (requestedAppDataDir) {
    app.setPath("userData", requestedAppDataDir);
    return requestedAppDataDir;
  }

  if (env.KANVIBE_QA_MODE === "1") {
    throw new Error("Automated QA requires an isolated KANVIBE_APP_DATA_DIR");
  }

  const isManualQa = app.commandLine.hasSwitch("qa");
  if (!app.isPackaged || isManualQa) {
    const directory = env.KANVIBE_RENDERER_URL && !isManualQa ? "kanvibe-dev" : "kanvibe-qa";
    const appDataDirectory = path.join(app.getPath("appData"), directory);
    app.setPath("userData", appDataDirectory);
    return appDataDirectory;
  }

  return app.getPath("userData");
}

module.exports = {
  applyAppDataDirectoryOverride,
  getRequestedAppDataDirectory,
  resolveHookServerPort,
};
