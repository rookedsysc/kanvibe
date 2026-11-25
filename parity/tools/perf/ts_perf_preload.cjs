/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * 성능 측정용 렌더러 preload(기본 session preload, isolated world). ts_perf_shim.cjs가 붙인다. TS 앱을 지울 때 함께 지운다.
 *
 * Rust `qa/perf_markers.rs`와 같은 시점, 곧 "화면 변화가 그려진 프레임의 다음 프레임 시작"에서 표식을 낸다.
 * Chromium은 프레임마다 rAF 콜백, 스타일·레이아웃, 그리기를 한 번에 처리한다(rendering update). 변화가 그 밖(작업 큐,
 * 예: React 커밋)에서 생겼으면 다음 프레임에 그려지므로 rAF를 두 번 기다리고, 그 안(다른 rAF 콜백, 예: xterm 줄 갱신)에서
 * 생겼으면 이번 프레임에 그려지므로 rAF를 한 번 기다린다. 어느 쪽인지는 기다리는 동안에만 도는 rAF 고리가 표시한다.
 * 고리 콜백은 프레임의 첫 rAF 콜백으로 돌며 깃발을 세우고, 같은 프레임의 그리기가 끝난 뒤 도는 작업에서 깃발을 내린다.
 */
const { ipcRenderer } = require("electron");

const MARKER_CHANNEL = "kanvibe-parity-perf:marker";
const FIRST_PAINT_MARKER_CHANNEL = "kanvibe-parity-perf:first-paint-marker";
const BOARD_CARD_SELECTOR = "[data-kanban-task-card='true'][data-kanban-status='todo']";
const TERMINAL_ROWS_SELECTOR = ".xterm-rows";
const TERMINAL_ECHO_TIMEOUT_MS = 3000;

function emit(name) {
  ipcRenderer.send(MARKER_CHANNEL, name, performance.timeOrigin + performance.now());
}

const renderingUpdate = { isInside: false, waiterCount: 0 };

function markRenderingUpdate() {
  if (renderingUpdate.waiterCount === 0) return;
  renderingUpdate.isInside = true;
  const channel = new MessageChannel();
  channel.port1.onmessage = () => {
    renderingUpdate.isInside = false;
  };
  channel.port2.postMessage(null);
  requestAnimationFrame(markRenderingUpdate);
}

function beginWaiting() {
  renderingUpdate.waiterCount += 1;
  if (renderingUpdate.waiterCount === 1) requestAnimationFrame(markRenderingUpdate);
}

function endWaiting() {
  renderingUpdate.waiterCount -= 1;
}

function afterFrames(count, callback) {
  requestAnimationFrame(() => (count <= 1 ? callback() : afterFrames(count - 1, callback)));
}

/** 방금 생긴 DOM 변화가 그려진 프레임의 다음 프레임 시작에서 표식을 낸다 */
function emitAfterPaint(name, onEmitted) {
  afterFrames(renderingUpdate.isInside ? 1 : 2, () => {
    emit(name);
    onEmitted();
  });
}

function isFirstPaintDrawn(markerName) {
  if (markerName === "first_board_paint") return Boolean(document.querySelector(BOARD_CARD_SELECTOR));
  const rows = document.querySelector(TERMINAL_ROWS_SELECTOR);
  return Boolean(rows && rows.textContent.trim());
}

/** 경로를 옮기는 세션은 첫 문서가 잠깐 보드를 그리므로, 기다릴 표식은 main shim이 목표 경로로 정해 준다 */
function watchFirstPaint() {
  const markerName = ipcRenderer.sendSync(FIRST_PAINT_MARKER_CHANNEL);
  beginWaiting();
  const observer = new MutationObserver(() => {
    if (!isFirstPaintDrawn(markerName)) return;
    observer.disconnect();
    emitAfterPaint(markerName, endWaiting);
  });
  observer.observe(document, { childList: true, subtree: true, characterData: true });
}

/** 커서 깜빡임은 줄을 다시 그려도 글이 같으므로 글이 바뀐 변화만 에코로 센다 */
function watchTerminalEcho(rows) {
  const textBefore = rows.textContent;
  beginWaiting();
  const observer = new MutationObserver(() => {
    if (rows.textContent === textBefore) return;
    observer.disconnect();
    clearTimeout(timer);
    emitAfterPaint("key_text_change", endWaiting);
  });
  const timer = setTimeout(() => {
    observer.disconnect();
    endWaiting();
  }, TERMINAL_ECHO_TIMEOUT_MS);
  observer.observe(rows, { childList: true, subtree: true, characterData: true });
}

function markKeystroke() {
  emit("key_received");
  afterFrames(2, () => emit("key_frame"));
  const rows = document.querySelector(TERMINAL_ROWS_SELECTOR);
  if (rows) watchTerminalEcho(rows);
}

// 다이어그램 워커 창(diagram-worker.html)에는 보드·터미널이 없으므로 앱 문서에서만 켠다
if (location.pathname.endsWith("/index.html")) {
  watchFirstPaint();
  window.addEventListener("keydown", markKeystroke, true);
}
