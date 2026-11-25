/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * 성능 측정용 Electron main shim. run_perf.py가 `electron -r <이 파일>`로 넣는다. TS 앱을 지울 때 이 디렉터리의 .cjs와 함께 지운다.
 *
 * 스크린샷 shim(고정 시각, 바깥 네트워크 차단, IPC 고장 주입 장치)을 그대로 불러 Rust `--qa`와 같은 결정성 조건을 맞춘다.
 * 그 위에 렌더러 표식 preload(ts_perf_preload.cjs)를 기본 session에 붙이고, preload가 보낸 표식을
 * stdout `KANVIBE_PERF <이름> <유닉스 epoch ns>`로 쓴다(Rust `qa/perf_markers.rs`와 같은 형식).
 * KANVIBE_PARITY_PERF_ROUTE가 있으면 첫 창이 처음 로드된 뒤 그 경로로 옮긴다(Rust `--route`에 해당).
 * 그 경우 첫 문서가 잠깐 보드를 그리므로, preload가 기다릴 첫 화면 표식을 경로로 정해 알려 준다.
 */
const path = require("node:path");

require("../screens/ts-driver/main-shim.cjs");

const MARKER_CHANNEL = "kanvibe-parity-perf:marker";
const FIRST_PAINT_MARKER_CHANNEL = "kanvibe-parity-perf:first-paint-marker";
const TASK_DETAIL_ROUTE_PREFIX = "/ko/task/";
const PRELOAD_PATH = path.join(__dirname, "ts_perf_preload.cjs");

function writeMarker(name, epochMs) {
  // 렌더러 시각은 소수 ms(µs 단위까지)라 µs로 반올림한 뒤 ns로 바꾼다
  const epochNs = BigInt(Math.round(epochMs * 1000)) * 1000n;
  process.stdout.write(`KANVIBE_PERF ${name} ${epochNs}\n`);
}

function moveFirstWindowToRoute(app, route) {
  let isRouteApplied = false;
  app.on("browser-window-created", (_event, window) => {
    window.webContents.on("did-finish-load", () => {
      const isAppDocument = window.webContents.getURL().includes("/index.html");
      if (isRouteApplied || !isAppDocument) return;
      isRouteApplied = true;
      void window.webContents.executeJavaScript(`window.location.hash = ${JSON.stringify(route)};`);
    });
  });
}

if (Boolean(process.versions.electron) && process.type === "browser") {
  const { app, ipcMain, session } = require("electron");
  // 앱의 whenReady 처리기(창 만들기)보다 먼저 등록되므로 첫 창부터 preload가 붙는다
  void app.whenReady().then(() => {
    session.defaultSession.setPreloads([...session.defaultSession.getPreloads(), PRELOAD_PATH]);
  });
  ipcMain.on(MARKER_CHANNEL, (_event, name, epochMs) => writeMarker(name, epochMs));
  const route = process.env.KANVIBE_PARITY_PERF_ROUTE;
  ipcMain.on(FIRST_PAINT_MARKER_CHANNEL, (event) => {
    event.returnValue = route?.startsWith(TASK_DETAIL_ROUTE_PREFIX) ? "first_terminal_rows" : "first_board_paint";
  });
  if (route) moveFirstWindowToRoute(app, route);
}
