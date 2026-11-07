#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync, spawnSync } = require('node:child_process');
const { launchKanVibeElectron } = require('../lib/launchElectron.cjs');

const MOD = process.platform === 'darwin' ? 'Meta' : 'Control';
const WINDOW_SHORTCUT = process.platform === 'darwin' ? 'Cmd+Shift+W' : 'Ctrl+Shift+W';
const root = process.cwd();
const runId = new Date().toISOString().replace(/[:.]/g, '-');
const out = path.join(root, 'qa-results/roky-harness/tab-close-runtime', runId);
const checks = [];
const dialogs = [];
const errors = [];
const processes = [];
const quote = (value) => `'${value.replace(/'/g, `'"'"'`)}'`;
const exec = (command, args, cwd) => execFileSync(command, args, { cwd, encoding: 'utf8', timeout: 10000 }).trim();
const tmux = (...args) => exec('tmux', args);
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (error) { if (error.code === 'ESRCH') return false; throw error; } };

function readPid(file) {
  const pid = Number(fs.readFileSync(file, 'utf8').trim());
  assert.ok(Number.isSafeInteger(pid) && pid > 1, `Invalid QA process PID: ${file}`);
  return pid;
}

async function until(read, predicate, label) {
  const deadline = Date.now() + 20000;
  let value;
  do {
    value = await read();
    if (predicate(value)) return value;
    await new Promise((resolve) => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  throw new Error(`${label}: ${JSON.stringify(value)}`);
}
async function step(name, action) {
  console.log(`START ${name}`);
  const detail = await action();
  checks.push({ name, ok: true, detail });
  console.log(`PASS ${name}`);
}
const invoke = (page, namespace, method, ...args) => page.evaluate(
  ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
  { namespace, method, args },
);
const tabs = (page, id) => invoke(page, 'terminalTabs', 'listTerminalTabs', id).then((result) => {
  assert.equal(result.ok, true);
  return result.tabs;
});

async function closeWithDialog(page, action, accept, expectedNames) {
  const seen = page.waitForEvent('dialog', { timeout: 10000 });
  const input = action().catch((error) => {
    if (!accept || !page.isClosed()) throw error;
  });
  const dialog = await seen;
  const message = dialog.message();
  dialogs.push({ type: dialog.type(), message, accept, at: new Date().toISOString() });
  try {
    assert.equal(dialog.type(), 'confirm');
    assert.ok(message.includes('프로세스가 종료될 수 있습니다'));
    assert.ok(message.includes(WINDOW_SHORTCUT));
    for (const name of expectedNames) assert.ok(message.includes(name), `${name} missing: ${message}`);
    if (accept) await dialog.accept(); else await dialog.dismiss();
  } catch (error) {
    await dialog.dismiss().catch(() => {});
    throw error;
  }
  await input;
  return message;
}

async function pressAndCloseWindow(page, shortcut) {
  const closed = page.waitForEvent('close');
  await page.keyboard.press(shortcut).catch((error) => {
    if (!page.isClosed()) throw error;
  });
  await closed;
}

async function run() {
  fs.mkdirSync(out, { recursive: true });
  const fixture = path.join(out, `tab-close-${runId}`);
  fs.mkdirSync(fixture);
  exec('git', ['init', '--initial-branch=main'], fixture);
  exec('git', ['config', 'user.name', 'KanVibe QA'], fixture);
  exec('git', ['config', 'user.email', 'qa@kanvibe.local'], fixture);
  fs.writeFileSync(path.join(fixture, 'README.md'), 'Isolated tab close QA fixture\n');
  exec('git', ['add', 'README.md'], fixture);
  exec('git', ['commit', '-m', 'Initial QA fixture'], fixture);
  const head = exec('git', ['rev-parse', 'HEAD'], root);
  const fingerprint = exec('git', ['diff', 'HEAD', '--binary'], root);
  let app;
  let board;
  let currentPage;
  let sessionName;
  let traceStarted = false;
  try {
    ({ app, page: board } = await launchKanVibeElectron({
      appDataDir: path.join(out, 'app-data'), outputDir: out,
      recordVideo: { dir: path.join(out, 'videos'), size: { width: 1440, height: 960 } },
    }));
    currentPage = board;
    assert.equal(await app.evaluate(({ app }) => app.getPath('userData')), path.join(out, 'app-data'));
    app.context().on('page', (page) => page.on('pageerror', (error) => errors.push(error.message)));
    await app.context().tracing.start({ screenshots: true, snapshots: true, sources: true });
    traceStarted = true;
    await board.waitForLoadState('domcontentloaded');
    try { await board.getByRole('button', { name: /닫기|Close/i }).first().click({ timeout: 2500 }); } catch {}
    const registered = await invoke(board, 'project', 'registerProject', `Tab Close QA ${runId}`, fixture);
    assert.equal(registered.success, true, registered.error);
    const projectId = registered.project.id;
    const plainId = await invoke(board, 'kanban', 'getTaskIdByProjectAndBranch', projectId, 'main');
    assert.equal((await invoke(board, 'kanban', 'connectTerminalSession', plainId, 'terminal')).sessionType, 'terminal');

    async function openDetail(id) {
      const next = app.waitForEvent('window', { timeout: 10000 });
      await board.evaluate((taskId) => {
        window.open(new URL(`#/ko/task/${taskId}`, location.href).href, "_blank");
      }, id);
      const page = await next;
      currentPage = page;
      await page.waitForLoadState('domcontentloaded');
      await page.evaluate((taskId) => { location.hash = `#/ko/task/${taskId}`; }, id);
      await page.getByTestId('terminal-tab-bar').waitFor({ timeout: 20000 });
      await page.mouse.click(1050, 650);
      await page.locator('.xterm-helper-textarea').last().waitFor({ state: 'attached' });
      return page;
    }
    async function startPlainProcess(page, id, label) {
      const active = (await tabs(page, id)).find((tab) => tab.isActive);
      const pidFile = path.join(out, `${label}.pid`);
      await page.locator('.xterm-helper-textarea').last().focus();
      await page.keyboard.type(`echo $$ > ${quote(pidFile)}; exec sleep 600`);
      await page.keyboard.press('Enter');
      await until(() => fs.existsSync(pidFile), Boolean, 'PID file not written');
      const pid = readPid(pidFile);
      processes.push(pid);
      await until(() => invoke(page, 'terminalTabs', 'getTerminalTabCloseInfo', id, active.id),
        (info) => info.processNames?.includes('sleep'), 'Current process not detected');
      assert.equal(alive(pid), true);
      return { tab: active, pid };
    }
    let page = await openDetail(plainId);
    await page.getByTestId('terminal-tab-new').click();
    await until(() => tabs(page, plainId), (list) => list.length === 2, 'Second tab missing');
    let worker = await startPlainProcess(page, plainId, 'plain-multi');
    await step('Cmd+W 취소: 프로세스 이름·창 닫기 안내 표시, 탭과 PID 유지', async () => {
      const message = await closeWithDialog(page, () => page.keyboard.press(`${MOD}+w`), false, ['sleep']);
      assert.equal((await tabs(page, plainId)).length, 2);
      assert.equal(alive(worker.pid), true);
      await page.screenshot({ path: path.join(out, 'plain-cancel.png') });
      return { message, pid: worker.pid };
    });
    await step('닫기 버튼 취소: 실제 프로세스와 탭 유지', async () => {
      await page.locator(`[data-terminal-tab-id='${worker.tab.id}']`).hover();
      await closeWithDialog(page, () => page.getByTestId(`terminal-tab-close-${worker.tab.id}`).click(), false, ['sleep']);
      assert.equal((await tabs(page, plainId)).length, 2);
      assert.equal(alive(worker.pid), true);
    });
    await step('Cmd+W 확인: 대상 탭 삭제와 실제 PID 종료', async () => {
      await closeWithDialog(page, () => page.keyboard.press(`${MOD}+w`), true, ['sleep']);
      await until(() => tabs(page, plainId), (list) => list.length === 1, 'Tab not deleted');
      await until(() => alive(worker.pid), (value) => !value, 'Process still running');
      await page.screenshot({ path: path.join(out, 'plain-confirm.png') });
    });
    worker = await startPlainProcess(page, plainId, 'plain-single');
    await step('일반 터미널 마지막 탭: 취소하면 PID 유지', async () => {
      await closeWithDialog(page, () => page.keyboard.press(`${MOD}+w`), false, ['sleep']);
      assert.equal((await tabs(page, plainId)).length, 1);
      assert.equal(alive(worker.pid), true);
    });
    await step('Cmd+Shift+W: 확인창 없이 창만 닫고 실제 PID 유지', async () => {
      const unexpected = [];
      page.on('dialog', async (dialog) => { unexpected.push(dialog.message()); await dialog.dismiss(); });
      await pressAndCloseWindow(page, `${MOD}+Shift+w`);
      assert.equal(alive(worker.pid), true);
      assert.deepEqual(unexpected, []);
    });
    page = await openDetail(plainId);
    await step('일반 터미널 마지막 탭 확인: 창 닫힘과 PID 종료', async () => {
      const closed = page.waitForEvent('close');
      await closeWithDialog(page, () => page.keyboard.press(`${MOD}+w`), true, ['sleep']);
      await closed;
      await until(() => alive(worker.pid), (value) => !value, 'Last process still running');
    });

    const task = await invoke(board, 'kanban', 'createTask', { title: `qa-close-${runId}`, projectId, baseBranch: 'main' });
    const expectedSessionName = `${path.basename(fixture)}-main`;
    assert.equal(spawnSync('tmux', ['has-session', '-t', expectedSessionName], { timeout: 10000 }).status, 1, 'QA session already exists');
    const connected = await invoke(board, 'kanban', 'connectTerminalSession', task.id, 'tmux');
    assert.equal(connected.sessionName, expectedSessionName);
    sessionName = expectedSessionName;
    page = await openDetail(task.id);
    const initial = await until(() => tabs(page, task.id), (list) => list.length === 1, 'Expected one tmux tab');
    const pidFile = path.join(out, 'tmux-worker.pid');
    const splitPidFile = path.join(out, 'tmux-split.pid');
    await page.getByTestId('terminal-tab-new').click();
    const createdTabs = await until(() => tabs(page, task.id), (list) => list.length === 2, 'tmux second tab missing');
    const windowId = createdTabs.find((tab) => tab.id !== initial[0].id).id;
    await page.locator(`[data-terminal-tab-id='${windowId}'][aria-selected='true']`).waitFor();
    tmux('send-keys', '-t', windowId, `echo $$ > ${quote(pidFile)}; exec sleep 600`, 'C-m');
    tmux('split-window', '-t', windowId, `echo $$ > ${quote(splitPidFile)}; exec tail -f /dev/null`);
    await until(() => tmux('list-panes', '-t', windowId, '-F', '#{pane_current_command}'),
      (commands) => commands.includes('sleep') && commands.includes('tail'), 'tmux foreground commands not ready');
    const pids = [pidFile, splitPidFile].map((file) => readPid(file));
    processes.push(...pids);
    await step('tmux 분할 화면: sleep·tail 표시 후 취소하면 두 PID 유지', async () => {
      await closeWithDialog(page, () => page.keyboard.press(`${MOD}+w`), false, ['sleep', 'tail']);
      assert.ok(pids.every(alive));
      assert.equal((await tabs(page, task.id)).length, 2);
      await page.screenshot({ path: path.join(out, 'tmux-cancel.png') });
    });
    await step('tmux 탭 닫기 확인: 두 분할 프로세스 종료', async () => {
      await closeWithDialog(page, () => page.keyboard.press(`${MOD}+w`), true, ['sleep', 'tail']);
      await until(() => tabs(page, task.id), (list) => list.length === 1, 'tmux window not closed');
      await until(() => pids.some(alive), (value) => !value, 'tmux processes still running');
    });
    await page.locator(`[data-terminal-tab-id='${initial[0].id}'][aria-selected='true']`).waitFor();
    const lastPidFile = path.join(out, 'tmux-last.pid');
    tmux('send-keys', '-t', initial[0].id, `echo $$ > ${quote(lastPidFile)}; exec sleep 600`, 'C-m');
    await until(() => fs.existsSync(lastPidFile), Boolean, 'Last tmux PID missing');
    const lastPid = readPid(lastPidFile);
    processes.push(lastPid);
    await step('tmux 마지막 탭 Cmd+W: 확인창 없이 창 닫기, 세션·탭·PID 유지', async () => {
      const unexpected = [];
      page.on('dialog', async (dialog) => { unexpected.push(dialog.message()); await dialog.dismiss(); });
      await pressAndCloseWindow(page, `${MOD}+w`);
      tmux('has-session', '-t', sessionName);
      assert.ok(tmux('list-windows', '-t', sessionName, '-F', '#{window_id}').includes(initial[0].id));
      assert.equal(alive(lastPid), true);
      assert.deepEqual(unexpected, []);
      return { lastPid, sessionName };
    });
    assert.deepEqual(errors, []);
  } catch (error) {
    checks.push({ name: 'runtime failure', ok: false, detail: error.stack });
    console.error(error);
    if (currentPage && !currentPage.isClosed()) await currentPage.screenshot({ path: path.join(out, 'failure.png') }).catch(() => {});
    process.exitCode = 1;
  } finally {
    if (traceStarted) await app.context().tracing.stop({ path: path.join(out, 'trace.zip') }).catch(() => {});
    const cleanupErrors = [];
    if (sessionName) {
      try { tmux('kill-session', '-t', sessionName); } catch (error) {
        if (spawnSync('tmux', ['has-session', '-t', sessionName], { timeout: 10000 }).status !== 1) cleanupErrors.push(error.message);
      }
      if (spawnSync('tmux', ['has-session', '-t', sessionName], { timeout: 10000 }).status !== 1) cleanupErrors.push('QA tmux session still exists');
    }
    if (app) {
      fs.writeFileSync(path.join(out, 'electron.log'), app.output());
      await app.close();
    }
    for (const pid of processes.filter(alive)) {
      try { process.kill(pid, 'SIGTERM'); } catch (error) {
        if (error.code !== 'ESRCH') cleanupErrors.push(error.message);
      }
    }
    try { await until(() => processes.filter(alive), (pids) => pids.length === 0, 'QA processes still running'); }
    catch (error) { cleanupErrors.push(error.message); }
    checks.push({ name: 'QA 세션·프로세스 정리 확인', ok: cleanupErrors.length === 0, detail: cleanupErrors });
    const unchanged = fingerprint === exec('git', ['diff', 'HEAD', '--binary'], root);
    const result = { head, unchanged, platform: process.platform, checks, dialogs, errors, out,
      ok: checks.length === 10 && checks.every((check) => check.ok) && unchanged,
      recording: 'Playwright captures actual renderer frames. Native confirmation UI may be absent in video; actual dialog events and text are retained in trace and result.json.' };
    fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2));
    console.log(`REPORT ${path.join(out, 'result.json')}`);
    if (!result.ok) process.exitCode = 1;
  }
}
run().catch((error) => { console.error(error); process.exitCode = 1; });
