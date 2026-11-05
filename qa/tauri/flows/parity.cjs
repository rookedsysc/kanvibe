const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const { launchKanVibeTauri } = require("../lib/launchTauri.cjs");

async function main() {
  const runDir = path.resolve(process.argv[2] || `qa-results/tauri-${Date.now()}`);
  if (fs.existsSync(path.join(runDir, "app-data"))) throw new Error("Use a fresh QA run directory");
  fs.mkdirSync(runDir, { recursive: true });
  const repo = path.join(runDir, "repository");
  fs.mkdirSync(repo);
  const git = (...args) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8" });
  git("init", "-b", "main"); git("config", "user.name", "KanVibe QA"); git("config", "user.email", "qa@kanvibe.local");
  fs.writeFileSync(path.join(repo, "README.md"), "# Native parity fixture\n");
  git("add", "."); git("commit", "-m", "Native QA fixture");
  const started = performance.now();
  const { app, page } = await launchKanVibeTauri({ appDataDir: path.join(runDir, "app-data"), hookPort: 19738 });
  const checks = [];
  const record = (name, details) => { checks.push({ name, ok: true, details }); console.log(`PASS ${name}`); };
  const invoke = (namespace, method, ...args) => page.evaluate(({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args), { namespace, method, args });
  const navigate = async route => {
    await page.evaluate(route => { location.hash = route; }, route);
    await new Promise(resolve => setTimeout(resolve, 350));
  };
  try {
    record("native startup", { ms: performance.now() - started });
    await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("qa_resize", { width: 1440, height: 960 }));
    await invoke("appSettings", "setBackgroundSyncEnabled", false);
    await invoke("appSettings", "setDefaultSessionType", "terminal");
    await invoke("appSettings", "setAppSetting", "theme_preference", "light");
    const registered = await invoke("project", "registerProject", "Native parity", repo);
    assert.equal(registered.success, true); assert.ok(registered.project.id);
    const projectId = registered.project.id;
    const task = await invoke("kanban", "createTask", { title: "Native QA task", description: "한글 입력과 UI parity", baseBranch: "main", projectId });
    assert.equal(task.title, "Native QA task");
    for (const status of ["progress", "pending", "review", "done", "todo"]) {
      const changed = await invoke("kanban", "updateTaskStatus", task.id, status);
      assert.equal(changed.status, status);
    }
    assert.equal((await invoke("kanban", "getTaskById", task.id)).project.id, projectId);
    record("SQLite task CRUD and all status transitions");
    const pairing = await invoke("mobileBridge", "startMobilePairing");
    const paired = await fetch("http://127.0.0.1:19738/api/mobile/pair", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code: pairing.code, deviceName: "session recovery QA" }) }).then(r => r.json());
    assert.equal(paired.success, true);
    const mobile = async (id) => {
      const response = await fetch(`http://127.0.0.1:19738/api/mobile/tasks/${id}/surfaces`, { headers: { authorization: `Bearer ${paired.token}` } });
      return { status: response.status, body: await response.json() };
    };
    for (const fields of [{}, { sessionType: "terminal" }, { sessionName: "stale-name" }]) {
      const pending = await invoke("kanban", "createTask", { title: "Mobile recovery", baseBranch: "main", projectId, ...fields });
      const responses = await Promise.all([mobile(pending.id), mobile(pending.id)]);
      for (const result of responses) { assert.equal(result.status, 200); assert.equal(result.body.surfaces.sessionType, "terminal"); }
      const saved = await invoke("kanban", "getTaskById", pending.id);
      assert.equal(saved.sessionType, "terminal"); assert.ok(saved.sessionName);
      await invoke("kanban", "deleteTask", pending.id);
    }
    const disconnected = await invoke("kanban", "createTask", { title: "No mobile session" });
    const unavailable = await mobile(disconnected.id);
    assert.equal(unavailable.status, 409); assert.equal(unavailable.body.reason, "no-session");
    await invoke("kanban", "deleteTask", disconnected.id);
    record("mobile surfaces recovers all missing-session combinations and retains no-session guard");

    await navigate("/ko");
    await page.waitForFunction(() => document.body.innerText.includes("Native QA task"));
    await page.evaluate(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "n", metaKey: true, bubbles: true, cancelable: true })));
    await page.waitForFunction(() => !!document.querySelector("input[name='branchName']"));
    assert.equal(await page.evaluate(() => document.querySelector("input[name='branchName']").value), "");
    await page.screenshot({ path: path.join(runDir, "create-modal.png") });
    await page.evaluate(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    record("app shortcut opens unchanged create-task modal");
    for (const [name, route] of [["board", "/ko"], ["settings", "/ko/settings"], ["shortcuts", "/ko/settings/shortcuts"], ["pane-layout", "/ko/pane-layout"], ["task", `/ko/task/${task.id}`]]) {
      await navigate(route);
      await page.waitForFunction(() => document.body.innerText.length > 20);
      await page.evaluate(() => { document.documentElement.dataset.theme = "light"; document.documentElement.style.colorScheme = "light"; });
      await page.screenshot({ path: path.join(runDir, `${name}.png`) });
      record(`actual WebView screenshot: ${name}`);
    }
    const connected = await invoke("kanban", "connectTerminalSession", task.id, "terminal");
    assert.equal(connected.sessionType, "terminal");
    const tabs = await invoke("terminalTabs", "listTerminalTabs", task.id);
    assert.ok(tabs.tabs.length > 0);
    const tabId = tabs.tabs[0].id;
    await page.evaluate(({ taskId, tabId }) => {
      window.__qaTerminalOutput = "";
      window.__qaDispose = window.kanvibeDesktop.onTerminalData(event => {
        if (event.taskId === taskId && event.tabId === tabId) window.__qaTerminalOutput += event.data;
      });
    }, { taskId: task.id, tabId });
    const opened = await page.evaluate(({ taskId, tabId }) => window.kanvibeDesktop.openTerminal(taskId, tabId, 90, 28), { taskId: task.id, tabId });
    assert.equal(opened.ok, true);
    await page.evaluate(({ taskId, tabId }) => {
      window.kanvibeDesktop.resizeTerminal(taskId, tabId, 100, 30);
      window.kanvibeDesktop.writeTerminal(taskId, tabId, "printf 'NATIVE_PARITY_한글\\n'\r");
    }, { taskId: task.id, tabId });
    await page.waitForFunction(() => window.__qaTerminalOutput.includes("NATIVE_PARITY_한글"));
    assert.equal((await invoke("terminalTabs", "createTerminalTab", task.id)).ok, true);
    const next = await invoke("terminalTabs", "listTerminalTabs", task.id);
    assert.ok(next.tabs.length > tabs.tabs.length);
    await page.evaluate(() => window.__qaDispose());
    record("native PTY input/output UTF8 resize and tabs");
    await page.evaluate(() => window.kanvibeDesktop.showNotification({ title: "Native notification QA", body: "fixture", locale: "ko", dedupeKey: "native-qa" }));
    const notifications = await page.evaluate(() => window.kanvibeDesktop.listNotifications());
    assert.ok(notifications.some(entry => entry.title === "Native notification QA"));
    record("native notification persisted and listed");
    const timings = [];
    for (let i = 0; i < 30; i++) { const time = performance.now(); await invoke("kanban", "getTasksByStatus"); timings.push(performance.now() - time); }
    record("board round-trip measurements", timings);
    await invoke("kanban", "deleteTask", task.id);
    assert.equal(await invoke("kanban", "getTaskById", task.id), null);
    record("task deletion with native session cleanup");
    fs.writeFileSync(path.join(runDir, "result.json"), JSON.stringify({ ok: true, checks }, null, 2));
  } catch (error) {
    fs.writeFileSync(path.join(runDir, "result.json"), JSON.stringify({ ok: false, checks, error: error.stack }, null, 2));
    throw error;
  } finally {
    fs.writeFileSync(path.join(runDir, "runtime.log"), app.output());
    await app.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
