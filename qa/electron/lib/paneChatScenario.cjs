const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");

async function until(read, accept, label) {
  const deadline = Date.now() + 30000;
  let value;
  while (Date.now() < deadline) {
    value = await read();
    if (accept(value)) return value;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`${label}: ${JSON.stringify(value)}`);
}

/** 실제 PTY·프로세스 탐색·IPC를 통과하며 외부 모델 API 없이 제어 동작을 검증한다. */
async function runPaneChatScenario({ page, invoke, taskId, repo, fakeHome, runDir }) {
  const executableDir = path.join(fakeHome, "fixture-bin");
  fs.mkdirSync(executableDir, { recursive: true });
  for (const provider of ["codex", "claude"]) {
    fs.copyFileSync(path.join(__dirname, "pane-chat-cli.py"), path.join(executableDir, provider));
  }
  // 모델 설정 선택창이 읽는 Codex 모델 목록. 한 모델에는 고를 옵션을 주고 다른 모델에는 주지 않아 두 경우를 모두 볼 수 있게 한다
  fs.mkdirSync(path.join(fakeHome, ".codex"), { recursive: true });
  fs.writeFileSync(path.join(fakeHome, ".codex", "models_cache.json"), JSON.stringify({ models: [
    { slug: "fixture-fast", display_name: "Fixture Fast", visibility: "list", priority: 1, supported_reasoning_levels: [{ effort: "low" }, { effort: "medium" }, { effort: "high" }], service_tiers: [{ id: "priority", name: "Fast" }] },
    { slug: "fixture-deep", display_name: "Fixture Deep", visibility: "list", priority: 2, supported_reasoning_levels: [], service_tiers: [] },
  ] }));
  const quote = (text) => "'" + text.replace(/'/g, "'\"'\"'") + "'";
  const write = (tabId, input) => page.evaluate(({ taskId, tabId, input }) => {
    window.kanvibeDesktop.writeTerminal(taskId, tabId, input);
  }, { taskId, tabId, input });
  const read = () => invoke("paneChat", "getPaneChatSnapshot", taskId);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.evaluate(async (taskId) => {
    const { navigateToTaskDetail } = await import("/src/desktop/renderer/utils/taskNavigation.ts");
    await navigateToTaskDetail(taskId, { currentLocale: "ko" });
  }, taskId);
  await page.keyboard.press("Escape");
  const closeUpdate = page.locator("button[aria-label*='업데이트'][aria-label*='닫기']").first();
  if (await closeUpdate.isVisible().catch(() => false)) await closeUpdate.click();
  await page.locator(".xterm").first().waitFor({ state: "visible", timeout: 30000 });
  const tabs = await until(() => invoke("terminalTabs", "listTerminalTabs", taskId), (result) => result.tabs.some((entry) => entry.isActive), "active task tab");
  const first = tabs.tabs.find((entry) => entry.isActive);
  assert.ok(first);
  await until(async () => invoke("mobileBridge", "getTaskSurfaces", taskId), (surfaces) => surfaces.tabs.some((tab) => tab.id === first.id), "first PTY");
  await write(first.id, `/usr/bin/python3 ${quote(path.join(executableDir, "codex"))} codex live-pane-a\r`);
  const initial = await until(read, (snapshot) => snapshot.provider === "codex" && snapshot.inputState === "ready", "Codex foreground");
  assert.equal(initial.paneId, first.id);
  // 출시 확인이 늦게 끝나면 업데이트 대화상자가 이 시점에 떠서 토글을 가린다. 뜰 시간을 준 뒤 한 번 더 닫는다
  await page.waitForTimeout(3000);
  await page.keyboard.press("Escape");
  if (await closeUpdate.isVisible().catch(() => false)) await closeUpdate.click({ position: { x: 5, y: 5 } });
  await page.getByTestId("pane-chat-toggle").click();
  const input = page.getByRole("textbox", { name: "현재 CLI에 메시지 보내기…" });
  await input.waitFor({ state: "visible" });
  await input.fill("Pane A first line\nPane A second line");
  await page.getByRole("button", { name: "보내기", exact: true }).click();
  await page.getByText("live-pane-a received on fixture-fast: Pane A first line", { exact: false }).first().waitFor({ state: "visible", timeout: 30000 });
  await page.screenshot({ path: path.join(runDir, "desktop-live-chat.png"), animations: "disabled" });
  await until(read, (snapshot) => snapshot.inputState === "ready", "prompt accepted");
  await page.getByRole("button", { name: "모델 선택", exact: true }).click();
  await page.getByTestId("pane-chat-controls").getByText("Select model", { exact: false }).waitFor({ state: "visible" });
  await page.screenshot({ path: path.join(runDir, "desktop-model-picker.png"), animations: "disabled" });
  await page.getByRole("button", { name: "아래로", exact: true }).click();
  await until(read, (snapshot) => snapshot.screen.includes("› 2. fixture-deep"), "model highlight");
  await page.getByRole("button", { name: "선택 / 확인", exact: true }).click();
  await until(read, (snapshot) => snapshot.inputState === "ready" && snapshot.screen.includes("model: fixture-deep"), "model confirmed");
  await page.getByRole("button", { name: "조작창 접기", exact: true }).click();
  await input.fill("After changing the model");
  await page.getByRole("button", { name: "보내기", exact: true }).click();
  await page.getByText("live-pane-a received on fixture-deep: After changing the model", { exact: true }).waitFor({ state: "visible", timeout: 30000 });
  await page.screenshot({ path: path.join(runDir, "desktop-model-response.png"), animations: "disabled" });
  await page.getByRole("button", { name: "터미널로 전환", exact: true }).first().click();
  await page.getByRole("button", { name: "새 탭", exact: true }).click();
  const second = await until(async () => invoke("terminalTabs", "listTerminalTabs", taskId), (result) => result.tabs.some((tab) => tab.id !== first.id && tab.isActive), "second tab");
  const secondTab = second.tabs.find((tab) => tab.id !== first.id && tab.isActive);
  await page.locator(".xterm").last().waitFor({ state: "visible" });
  await write(secondTab.id, `/usr/bin/python3 ${quote(path.join(executableDir, "claude"))} claude live-pane-b\r`);
  // Claude와 Codex의 실제 prompt 표식은 서로 다르다.
  await until(read, (snapshot) => snapshot.provider === "claude" && snapshot.inputState === "ready", "Claude foreground");
  const stale = await invoke("paneChat", "sendPaneChatPrompt", taskId, initial.targetToken, "Must never reach B").then(() => null, (error) => error.message);
  assert.match(stale, /변경|changed/i);
  await page.getByTestId("pane-chat-toggle").click();
  const secondSnapshot = await read();
  assert.equal(secondSnapshot.paneId, secondTab.id);
  assert.equal(secondSnapshot.provider, "claude");
  await input.fill("Only pane B");
  await page.getByRole("button", { name: "보내기", exact: true }).click();
  await page.getByText("live-pane-b received on fixture-fast: Only pane B", { exact: true }).waitFor({ state: "visible", timeout: 30000 });
  const receivedA = Buffer.from(fs.readFileSync(path.join(repo, "live-pane-a-input.jsonl"), "utf8").trim().split("\n").flatMap((line) => JSON.parse(line).bytes)).toString("utf8");
  const receivedB = Buffer.from(fs.readFileSync(path.join(repo, "live-pane-b-input.jsonl"), "utf8").trim().split("\n").flatMap((line) => JSON.parse(line).bytes)).toString("utf8");
  assert.equal(receivedA.includes("Only pane B"), false);
  assert.equal(receivedB.includes("Must never reach B"), false);
  await page.screenshot({ path: path.join(runDir, "desktop-second-pane.png"), animations: "disabled" });
  const result = { pass: true, firstPaneId: first.id, secondPaneId: secondTab.id, checks: [
    "real foreground process targeting", "multiline PTY delivery", "exact JSONL transcript",
    "native model picker navigation and confirmation", "response uses selected fixture model",
    "switch preserves existing PTYs", "stale focus token rejected before input",
  ], providerApi: "deterministic CLI fixtures; no paid model request" };
  fs.writeFileSync(path.join(runDir, "pane-chat-result.json"), JSON.stringify(result, null, 2));
  return result;
}
module.exports = { runPaneChatScenario };
