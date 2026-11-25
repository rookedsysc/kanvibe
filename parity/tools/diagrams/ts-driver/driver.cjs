/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * 다이어그램 기준 이미지용 Electron main 진입점이다. TS 앱을 지울 때 디렉터리째 지운다.
 *
 * capture.py가 만든 임시 앱 디렉터리(build·electron은 저장소로 가는 링크)에서 이 파일을 main으로 실행한다.
 * 모바일 경로와 똑같이 빌드된 renderDiagramInDesktop()을 불러 JPEG를 받고, 같은 diagram-worker.html을
 * 보조 창에 한 번 더 띄워 Mermaid가 만든 SVG 원문을 받아 둔다. SVG는 네이티브 렌더러의 좌표를 맞출 때 쓴다.
 * 항목의 configuration(mobile·desktop)에 맞는 워커 문서를 그리기 전마다 diagram-worker.html 자리에 놓는다.
 * 모바일 항목은 제품이 실제로 보내는 JPEG(toJPEG(100))를 그대로 남긴다. 데스크톱 화면은 SVG를 DOM에 바로 그려 JPEG 단계가
 * 없으므로, 데스크톱 항목에서는 같은 프레임을 손실 없는 PNG로 받는다.
 */
const fs = require("node:fs");
const path = require("node:path");
const { app, BrowserWindow, ipcMain } = require("electron");

const jobPath = process.argv[process.argv.length - 1];
const job = JSON.parse(fs.readFileSync(jobPath, "utf8"));

function captureSvg(input) {
  const rendererRoot = path.join(app.getAppPath(), "build", "renderer");
  const window = new BrowserWindow({
    show: false, width: input.width, height: 2400, useContentSize: true,
    webPreferences: { preload: path.join(__dirname, "svg-preload.cjs"), sandbox: false, contextIsolation: true, offscreen: true },
  });
  return new Promise((resolve) => {
    const timeout = setTimeout(() => finish(null), 15000);
    function finish(svg) {
      clearTimeout(timeout);
      ipcMain.removeAllListeners("kanvibe:diagram:ready");
      ipcMain.removeAllListeners("kanvibe:diagram:svg");
      if (!window.isDestroyed()) window.destroy();
      resolve(svg);
    }
    ipcMain.once("kanvibe:diagram:ready", (event) => event.sender.send("kanvibe:diagram:render", input));
    ipcMain.once("kanvibe:diagram:svg", (_event, svg) => finish(svg));
    window.loadFile(path.join(rendererRoot, "diagram-worker.html")).catch(() => finish(null));
  });
}

// 데스크톱 항목이면 워커 창의 paint 이벤트가 넘기는 프레임을 감싸, 워커가 부르는 crop(...).toJPEG(100)이 같은 그림의 PNG를
// 돌려주게 한다. NativeImage 메서드는 프로토타입을 바꿔도 먹지 않아 이벤트 쪽에서 감싼다
let isLosslessCapture = false;

function losslessFrame(frame) {
  return {
    crop(rectangle) {
      const cropped = frame.crop(rectangle);
      return {
        toBitmap: (options) => cropped.toBitmap(options),
        toJPEG: () => cropped.toPNG(),
      };
    },
  };
}

app.on("web-contents-created", (_event, contents) => {
  const once = contents.once.bind(contents);
  contents.once = (name, listener) => {
    if (name !== "paint" || !isLosslessCapture) return once(name, listener);
    return once(name, (paintEvent, dirty, frame) => listener(paintEvent, dirty, losslessFrame(frame)));
  };
});

/** capture.py가 만든 설정별 워커 문서(diagram-worker.mobile.html·diagram-worker.desktop.html)를 고른다 */
function useWorkerConfiguration(configuration) {
  const rendererRoot = path.join(app.getAppPath(), "build", "renderer");
  fs.copyFileSync(path.join(rendererRoot, `diagram-worker.${configuration}.html`), path.join(rendererRoot, "diagram-worker.html"));
}

// 다이어그램 창을 닫을 때마다 앱이 끝나지 않게 한다
app.on("window-all-closed", () => {});

async function run() {
  await app.whenReady();
  const worker = require(path.join(job.repoRoot, "build", "main", "src", "desktop", "main", "mobileDiagramWorker.js"));
  const results = [];
  for (const entry of job.entries) {
    const input = { definition: entry.definition, theme: entry.theme, width: entry.width };
    const result = { id: entry.id, theme: entry.theme, width: entry.width };
    useWorkerConfiguration(entry.configuration);
    const isLossless = entry.configuration === "desktop";
    isLosslessCapture = isLossless;
    try {
      const rendered = await worker.renderDiagramInDesktop(input);
      const extension = isLossless ? "png" : "jpg";
      fs.writeFileSync(path.join(job.outDir, `${entry.id}--${entry.theme}.${extension}`), rendered.encoded);
      result.naturalWidth = rendered.naturalWidth ?? null;
      result.success = true;
    } catch (error) {
      result.success = false;
      result.statusCode = error.statusCode ?? null;
      result.error = String(error.message ?? error);
    }
    const svg = await captureSvg(input);
    if (svg) fs.writeFileSync(path.join(job.outDir, `${entry.id}--${entry.theme}.svg`), svg);
    results.push(result);
  }
  fs.writeFileSync(path.join(job.outDir, "results.json"), JSON.stringify(results, null, 2));
  app.exit(0);
}

run().catch((error) => {
  process.stderr.write(`${error.stack ?? error}\n`);
  app.exit(1);
});
