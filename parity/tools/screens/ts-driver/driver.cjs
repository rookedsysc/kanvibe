#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * Electron(TS) 앱용 스크린샷 드라이버. TS 앱을 지울 때 이 디렉터리째 지운다.
 *
 * capture.py가 만든 job JSON(argv[2])을 읽어 앱을 띄우고 states.yaml의 단계를 실행한다.
 * 화면이 준비되면 stdout에 {"event":"ready"}를 한 줄 쓰고, capture.py가 x11grab으로 찍은 뒤
 * stdin에 "captured"를 보내면 앱을 닫는다. 캡처 자체는 이 드라이버가 하지 않는다(GPUI 앱과 같은 캡처 경로를 쓰기 위해).
 */
const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");

const REPO_ROOT = path.resolve(__dirname, "../../../..");
const { _electron: electron } = require(path.join(REPO_ROOT, "node_modules/@playwright/test"));
const electronExecutable = require(path.join(REPO_ROOT, "node_modules/electron"));

const RENDERER_INDEX_URL = `file://${path.join(REPO_ROOT, "build/renderer/index.html")}`;

/** 애니메이션·전환·텍스트 커서를 꺼서 같은 상태가 항상 같은 픽셀로 그려지게 한다 */
const STABILIZE_CSS = `
*, *::before, *::after {
  animation: none !important;
  transition: none !important;
  caret-color: transparent !important;
  scroll-behavior: auto !important;
}
.monaco-editor .cursors-layer > .cursor {
  visibility: hidden !important;
}
`;

function emit(event) {
  process.stdout.write(`${JSON.stringify(event)}\n`);
}

function log(message) {
  process.stderr.write(`[ts-driver] ${message}\n`);
}

function waitForStdinLine() {
  return new Promise((resolve) => {
    const reader = readline.createInterface({ input: process.stdin });
    reader.once("line", (line) => {
      reader.close();
      resolve(line.trim());
    });
  });
}

function withTimeout(promise, ms, label) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label} timed out after ${ms}ms`)), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}

function toPlaywrightKey(key) {
  return key.replace(/\bMod\b/g, "Control");
}

/** states.yaml의 대상 기술(텍스트·역할·라벨·선택자)을 Playwright locator로 바꾼다 */
function resolveLocator(page, target) {
  if (typeof target === "string") {
    return page.getByText(target, { exact: true }).first();
  }

  let locator;
  if (target.role) {
    const options = {};
    if (target.name !== undefined) options.name = target.name;
    if (target.exact !== undefined) options.exact = target.exact;
    locator = page.getByRole(target.role, options);
  } else if (target.label) {
    locator = page.getByLabel(target.label, { exact: target.exact ?? false });
  } else if (target.placeholder) {
    locator = page.getByPlaceholder(target.placeholder, { exact: target.exact ?? false });
  } else if (target.text) {
    locator = page.getByText(target.text, { exact: target.exact ?? true });
  } else if (target.title) {
    locator = page.getByTitle(target.title, { exact: target.exact ?? false });
  } else if (target.selector) {
    locator = page.locator(target.selector);
  } else {
    throw new Error(`Unsupported target: ${JSON.stringify(target)}`);
  }

  if (target.within) {
    locator = resolveLocator(page, target.within).locator(locator);
  }
  if (target.nth === "last") return locator.last();
  return locator.nth(target.nth ?? 0);
}

async function emitBoardEvent(app, payload) {
  await app.evaluate(({ BrowserWindow }, eventPayload) => {
    for (const window of BrowserWindow.getAllWindows()) {
      window.webContents.send("kanvibe:board-event", eventPayload);
    }
  }, payload);
}

async function setFault(app, target, fault) {
  await app.evaluate((_electron, { faultTarget, faultValue }) => {
    const faults = globalThis.__kanvibeParity.faults;
    if (faultValue === null) {
      delete faults[faultTarget];
      return;
    }
    const existing = faults[faultTarget];
    faults[faultTarget] = existing ? [...(Array.isArray(existing) ? existing : [existing]), faultValue] : faultValue;
  }, { faultTarget: target, faultValue: fault });
}

const SERVICE_RESULTS_PATH = path.join(REPO_ROOT, "parity/fixtures/service-results.json");

function toShimFault(value) {
  const resultValue = value.value_ref
    ? JSON.parse(fs.readFileSync(SERVICE_RESULTS_PATH, "utf8")).values[value.value_ref]
    : value.value;
  return { mode: value.mode, message: value.message, ms: value.ms, value: resultValue, afterCalls: value.after_calls, match: value.match };
}

async function runStep(context, step) {
  const { app, page } = context;
  const [kind] = Object.keys(step);
  const value = step[kind];
  log(`step ${kind} ${JSON.stringify(value)}`);

  switch (kind) {
    case "wait":
      await page.waitForTimeout(value);
      return;
    case "wait_for":
      await resolveLocator(page, value).waitFor({ state: value.state ?? "visible", timeout: value.timeout ?? 15000 });
      return;
    case "click":
      await resolveLocator(page, value).click({ position: value.position, force: value.force });
      return;
    case "double_click":
      await resolveLocator(page, value).dblclick();
      return;
    case "right_click":
      await resolveLocator(page, value).click({ button: "right", position: value.position });
      return;
    case "hover":
      await resolveLocator(page, value).hover({ position: value.position });
      return;
    case "focus":
      await resolveLocator(page, value).focus();
      return;
    case "fill":
      await resolveLocator(page, value).fill(value.value);
      return;
    case "type":
      await page.keyboard.type(value, { delay: 20 });
      return;
    case "press":
      for (const key of Array.isArray(value) ? value : [value]) {
        await page.keyboard.press(toPlaywrightKey(key));
      }
      return;
    case "mouse_move":
      await page.mouse.move(value.x, value.y);
      return;
    case "mouse_down":
      await page.mouse.down();
      return;
    case "mouse_up":
      await page.mouse.up();
      return;
    case "drag_hold": {
      // 대상을 집어 들고 목표 지점 위에 멈춘 채 둔다(드롭하지 않음)
      const source = await resolveLocator(page, value.from).boundingBox();
      const target = await resolveLocator(page, value.to).boundingBox();
      const startX = source.x + source.width / 2;
      const startY = source.y + Math.min(20, source.height / 2);
      await page.mouse.move(startX, startY);
      await page.mouse.down();
      await page.mouse.move(startX + 4, startY + 4, { steps: 2 });
      await page.waitForTimeout(200);
      await page.mouse.move(target.x + target.width / 2, target.y + (value.to.offset_y ?? 40), { steps: 12 });
      return;
    }
    case "scroll": {
      const locator = resolveLocator(page, value);
      await locator.evaluate((element, deltaY) => {
        element.scrollTop = deltaY === "bottom" ? element.scrollHeight : element.scrollTop + deltaY;
      }, value.delta_y);
      return;
    }
    case "scroll_into_view":
      await resolveLocator(page, value).scrollIntoViewIfNeeded();
      return;
    case "fault":
      await setFault(app, value.target, toShimFault(value));
      return;
    case "clear_fault":
      await setFault(app, value, null);
      return;
    case "board_event":
      await emitBoardEvent(app, value);
      return;
    case "main_send":
      await app.evaluate(({ BrowserWindow }, message) => {
        for (const window of BrowserWindow.getAllWindows()) {
          window.webContents.send(message.channel, message.payload);
        }
      }, value);
      return;
    case "navigate":
      await page.evaluate((route) => { window.location.hash = route; }, value);
      return;
    case "reload":
      await page.reload();
      return;
    case "eval":
      // 일반 단계로 표현할 수 없는 조작만 쓴다. states.yaml에 설명을 함께 적는다.
      await page.evaluate(value.script);
      return;
    case "set_window_size":
      await setWindowBounds(app, value.width, value.height);
      return;
    default:
      throw new Error(`Unknown step: ${kind}`);
  }
}

async function setWindowBounds(app, width, height) {
  await app.evaluate(({ BrowserWindow }, bounds) => {
    for (const window of BrowserWindow.getAllWindows()) {
      if (!window.isVisible()) continue;
      window.setBounds({ x: 0, y: 0, width: bounds.width, height: bounds.height });
      window.focus();
    }
  }, { width, height });
}

async function waitForStableFrame(page, settleMs, isJavaScriptDisabled) {
  await page.waitForTimeout(settleMs);
  if (isJavaScriptDisabled) return;
  await page.evaluate(() => document.fonts.ready.then(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  })));
}

/** 화면에 실제로 쓰인 글꼴(fontconfig가 고른 파일의 family)을 CDP로 모은다. fonts.md 근거 자료용이다 */
async function auditPlatformFonts(page, outputPath) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  const { root } = await cdp.send("DOM.getDocument", { depth: -1, pierce: true });
  const usage = {};
  const visit = async (node) => {
    if (node.nodeType === 1) {
      try {
        const { fonts } = await cdp.send("CSS.getPlatformFontsForNode", { nodeId: node.nodeId });
        for (const font of fonts) {
          const key = `${font.familyName}|${font.postScriptName ?? ""}|${font.isCustomFont ? "custom" : "system"}`;
          usage[key] = (usage[key] ?? 0) + font.glyphCount;
        }
      } catch {
        // 텍스트가 없는 노드는 건너뛴다.
      }
    }
    for (const child of node.children ?? []) await visit(child);
    if (node.contentDocument) await visit(node.contentDocument);
    for (const shadow of node.shadowRoots ?? []) await visit(shadow);
  };
  await visit(root);
  fs.writeFileSync(outputPath, JSON.stringify(usage, null, 2));
}

/**
 * Diff 화면의 Monaco는 런타임에 jsDelivr CDN에서 읽힌다. 네트워크를 막은 채로 실제 화면을 찍기 위해
 * 같은 버전(0.55.1)의 로컬 node_modules/monaco-editor/min을 그 URL로 돌려준다. 다른 바깥 요청은 모두 막힌다.
 */
const MONACO_CDN_PREFIX = "https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/";
const MONACO_LOCAL_DIR = path.join(REPO_ROOT, "node_modules/monaco-editor/min");
const CONTENT_TYPES = { ".js": "application/javascript", ".css": "text/css", ".ttf": "font/ttf", ".json": "application/json" };

async function serveMonacoFromNodeModules(page) {
  await page.context().route(`${MONACO_CDN_PREFIX}**`, async (route) => {
    const relativePath = decodeURIComponent(new URL(route.request().url()).pathname.slice(new URL(MONACO_CDN_PREFIX).pathname.length));
    const filePath = path.join(MONACO_LOCAL_DIR, relativePath);
    if (!filePath.startsWith(MONACO_LOCAL_DIR) || !fs.existsSync(filePath)) {
      await route.fulfill({ status: 404, body: "" });
      return;
    }
    await route.fulfill({
      status: 200,
      body: fs.readFileSync(filePath),
      headers: { "content-type": CONTENT_TYPES[path.extname(filePath)] ?? "application/octet-stream", "access-control-allow-origin": "*" },
    });
  });
}

async function prepareRenderer(page, job) {
  await serveMonacoFromNodeModules(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.clock.setFixedTime(new Date(job.fixedNowMs));
  await page.addInitScript(({ css, platform, dialogs }) => {
    // Electron의 window.confirm/alert는 네이티브 GTK 대화상자로 떠서 렌더러를 멈춘다.
    // 상태 재현에서는 확인 대화상자를 수락(기본) 또는 거절한 것으로 처리한다.
    window.confirm = () => dialogs !== "dismiss";
    window.alert = () => {};
    if (platform === "mac") {
      Object.defineProperty(Navigator.prototype, "platform", { get: () => "MacIntel" });
      Object.defineProperty(Navigator.prototype, "userAgent", {
        get: () => "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Electron/32.0.0 Safari/537.36",
      });
    }
    const install = () => {
      const style = document.createElement("style");
      style.setAttribute("data-parity", "stabilize");
      style.textContent = css;
      (document.head || document.documentElement).appendChild(style);
    };
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", install, { once: true });
    } else {
      install();
    }
  }, { css: STABILIZE_CSS, platform: job.state.platform ?? "linux", dialogs: job.state.dialogs ?? "accept" });
}

/**
 * URL이 정규식에 맞는 file:// 요청을 끝내지 않고 붙잡아 둔다. lazy 라우트 번들이 영원히 로딩 중인 상태를 만들 때 쓴다.
 * CDP Fetch는 file://을 가로채지 못하므로 Electron main의 protocol.handle로 처리한다.
 */
async function holdRequests(app, urlRegexes) {
  if (!urlRegexes || urlRegexes.length === 0) return;
  await app.evaluate(({ net, session }, patterns) => {
    const regexes = patterns.map((pattern) => new RegExp(pattern));
    session.defaultSession.protocol.handle("file", (request) => {
      if (regexes.some((regex) => regex.test(request.url))) {
        // 참조 없는 promise는 GC되어 요청이 실패로 끝날 수 있으므로 resolve를 붙잡아 둔다
        globalThis.__kanvibeParityHeldRequests ??= [];
        return new Promise((resolve) => globalThis.__kanvibeParityHeldRequests.push(resolve));
      }
      return net.fetch(request, { bypassCustomProtocolHandlers: true });
    });
  }, urlRegexes);
}

async function navigateFresh(app, page, job) {
  await holdRequests(app, job.state.hold_requests);
  if (job.state.disable_javascript) {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setScriptExecutionDisabled", { value: true });
  }
  // 첫 창은 이미 #/ko를 그린 상태다. 같은 URL로 goto하면 문서를 다시 만들지 않아 시계·CSS 초기화 스크립트가 빠지므로
  // 빈 문서를 거쳐 항상 새 문서로 연다.
  await page.goto("about:blank");
  await setFault(app, "releaseUpdates.*", null);
  for (const fault of (job.state.faults ?? []).filter((entry) => entry.target.startsWith("releaseUpdates."))) {
    await setFault(app, fault.target, toShimFault(fault));
  }
  await page.goto(`${RENDERER_INDEX_URL}#${job.state.route}`);
  if (job.state.disable_javascript) {
    // 스크립트가 꺼져 초기화 스크립트로 CSS를 넣을 수 없으므로 CDP로 같은 안정화 CSS를 붙인다
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("DOM.enable");
    await cdp.send("CSS.enable");
    const { frameTree } = await cdp.send("Page.getFrameTree");
    const { styleSheetId } = await cdp.send("CSS.createStyleSheet", { frameId: frameTree.frame.id });
    await cdp.send("CSS.setStyleSheetText", { styleSheetId, text: STABILIZE_CSS });
  }
}

async function main() {
  const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
  const shimPath = path.join(__dirname, "main-shim.cjs");
  const env = {
    ...job.env,
    KANVIBE_PARITY_FIXED_NOW_MS: String(job.fixedNowMs),
    KANVIBE_PARITY_NETWORK_FIXTURE: job.networkFixturePath,
  };

  const app = await electron.launch({
    executablePath: electronExecutable,
    // Playwright는 NODE_OPTIONS를 지우므로 shim은 Electron의 -r 인자로 넣는다(Playwright 자신의 loader도 같은 방식이다).
    args: [
      "-r",
      shimPath,
      `--kanvibe-hook-port=${job.hookPort}`,
      "--no-sandbox",
      "--force-device-scale-factor=1",
      "--force-color-profile=srgb",
      "--disable-lcd-text",
      "--lang=en-US",
      "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE 127.0.0.1",
      REPO_ROOT,
    ],
    cwd: REPO_ROOT,
    env,
    timeout: 45000,
  });
  const child = app.process();
  child.stdout?.on("data", (chunk) => process.stderr.write(`[app] ${chunk}`));
  child.stderr?.on("data", (chunk) => process.stderr.write(`[app] ${chunk}`));

  let closeStartedAt = 0;
  try {
    // 첫 창은 드라이버가 준비를 마치기 전에 #/ko를 한 번 그린다. 그때 릴리스 확인이 돌면 main이 그 버전을
    // "이미 보여 줌"으로 기억해 실제 캡처 문서에서는 대화상자가 뜨지 않는다. 준비가 끝날 때까지 릴리스 확인을 붙잡아 둔다.
    await setFault(app, "releaseUpdates.*", { mode: "hang" });
    for (const fault of job.state.faults ?? []) {
      await setFault(app, fault.target, toShimFault(fault));
    }
    const mainClock = await app.evaluate(() => ({ now: new Date().toISOString(), shim: Boolean(globalThis.__kanvibeParity) }));
    log(`main clock ${JSON.stringify(mainClock)}`);
    const page = await app.firstWindow({ timeout: 45000 });
    page.setDefaultTimeout(15000);
    // 앱의 window.confirm(삭제 확인 등)은 기본으로 수락한다. 상태가 거절을 요구하면 dialogs: dismiss를 쓴다.
    page.on("dialog", (dialog) => {
      log(`dialog ${dialog.type()}: ${dialog.message()}`);
      void (job.state.dialogs === "dismiss" ? dialog.dismiss() : dialog.accept());
    });
    await setWindowBounds(app, job.window.width, job.window.height);
    await prepareRenderer(page, job);
    await navigateFresh(app, page, job);
    const context = { app, page, job };
    for (const step of job.state.steps ?? []) {
      await withTimeout(runStep(context, step), 30000, `step ${JSON.stringify(step)}`);
    }
    if (!job.state.keep_mouse) {
      await page.mouse.move(job.window.width - 1, job.window.height - 1).catch(() => {});
    }
    await waitForStableFrame(page, job.state.settle_ms ?? job.settleMs, Boolean(job.state.disable_javascript));
    if (job.fontAuditPath && !job.state.disable_javascript) await auditPlatformFonts(page, job.fontAuditPath).catch((error) => log(`font audit failed: ${error.message}`));
    emit({ event: "ready" });
    const reply = await withTimeout(waitForStdinLine(), 60000, "capture acknowledgement");
    if (reply !== "captured") throw new Error(`unexpected reply: ${reply}`);
  } finally {
    closeStartedAt = Date.now();
    // 다음 실행은 fixture를 새로 만들므로 정상 종료를 오래 기다리지 않는다. 남은 tmux 서버는 capture.py가 정리한다.
    await withTimeout(app.close(), 3000, "app.close").catch((error) => {
      log(`close failed: ${error.message}; killing the Electron process group`);
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch {
        child.kill("SIGKILL");
      }
    });
  }
  log(`app closed in ${Date.now() - closeStartedAt}ms`);
  emit({ event: "done" });
  process.exit(0);
}

main().catch((error) => {
  emit({ event: "error", message: error.stack || String(error) });
  process.exit(1);
});
