#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync, spawnSync } = require('node:child_process');
const { launchKanVibeTauri } = require('../lib/launchTauri.cjs');

const root = process.cwd();
const out = path.resolve(process.argv[2] || `qa-results/roky-harness/tab-close-runtime/${Date.now()}`);
const checks = [];
const dialogs = [];
const processes = [];
const quote = value => `'${value.replace(/'/g, `'"'"'`)}'`;
const exec = (command, args, cwd) => execFileSync(command, args, { cwd, encoding: 'utf8', timeout: 10000 }).trim();
const tmux = (...args) => exec('tmux', args);
const alive = pid => { try { process.kill(pid, 0); return true; } catch (error) { if (error.code === 'ESRCH') return false; throw error; } };
const invoke = (page, namespace, method, ...args) => page.evaluate(
  ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args), { namespace, method, args });
const tabs = async (page, id) => {
  const result = await invoke(page, 'terminalTabs', 'listTerminalTabs', id);
  assert.equal(result.ok, true, result.error);
  return result.tabs;
};
async function until(read, predicate, label) {
  const deadline = Date.now() + 20000;
  let value;
  do {
    value = await read();
    if (predicate(value)) return value;
    await new Promise(resolve => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  throw new Error(`${label}: ${JSON.stringify(value)}`);
}
async function step(name, action) {
  console.log(`START ${name}`);
  const detail = await action();
  checks.push({ name, ok: true, detail });
  console.log(`PASS ${name}`);
}
const windows = board => board.evaluate(() => window.__TAURI_INTERNALS__.invoke('plugin:window|get_all_windows'));
const key = (page, shiftKey = false) => page.evaluate(shiftKey => {
  const event = new KeyboardEvent('keydown', {
    key: 'w', code: 'KeyW', metaKey: navigator.userAgent.includes('Mac'),
    ctrlKey: !navigator.userAgent.includes('Mac'), shiftKey, bubbles: true, cancelable: true,
  });
  window.dispatchEvent(event);
  return {prevented:event.defaultPrevented,href:location.href,platform:navigator.platform,modals:[...document.querySelectorAll('[role=dialog]')].map(e=>e.textContent)};
}, shiftKey);

// Dialog button results are injected at the IPC boundary; actual OS dialogs require separate native UI QA.
async function closeWithDialog(page, action, accept, expectedNames, closesWindow = false) {
  await page.evaluate(accept => { window.__qaDialogDecision = accept ? 'Ok' : 'Cancel'; window.__qaDialogMessages = []; window.__qaDecisionDelivered = false; }, accept);
  await action();
  const messages = await until(() => page.evaluate(() => window.__qaDialogMessages), list => list.length > 0, 'Confirmation not requested');
  assert.equal(messages.length, 1);
  const message = messages[0];
  assert.ok(message.includes('프로세스가 종료될 수 있습니다'), message);
  assert.ok(message.includes(process.platform === 'darwin' ? 'Cmd+Shift+W' : 'Ctrl+Shift+W'), message);
  for (const name of expectedNames) assert.ok(message.includes(name), `${name} missing: ${message}`);
  dialogs.push({ message, accept });
  await page.evaluate(() => { setTimeout(() => { window.__qaResolveDialog(window.__qaDialogDecision); window.__qaDecisionDelivered = true; }, 10); });
  if (!closesWindow) {
    await until(() => page.evaluate(() => window.__qaDecisionDelivered), Boolean, 'Dialog decision not delivered');
    // A canceled native dialog resumes asynchronously; allow its route close guard to clear before the next command.
    await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 50)));
  }
  return message;
}
async function run() {
  if (fs.existsSync(path.join(out, 'app-data'))) throw new Error('Use a fresh QA run directory');
  fs.mkdirSync(out, { recursive: true });
  const fixture = path.join(out, `tab-close-${Date.now()}`);
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
  let sessionName;
  let error;
  try {
    ({ app, page: board } = await launchKanVibeTauri({ appDataDir: path.join(out, 'app-data'), hookPort: 19739 }));
    await invoke(board, 'appSettings', 'setBackgroundSyncEnabled', false);
    const registered = await invoke(board, 'project', 'registerProject', `Tab Close QA ${Date.now()}`, fixture);
    assert.equal(registered.success, true, registered.error);
    const projectId = registered.project.id;
    const plainId = await invoke(board, 'kanban', 'getTaskIdByProjectAndBranch', projectId, 'main');
    assert.equal((await invoke(board, 'kanban', 'changeTaskSessionType', plainId, 'terminal', true)).sessionType, 'terminal');
    async function openDetail(id) {
      const before = new Set(await windows(board));
      await board.evaluate(id => window.kanvibeDesktop.openInternalRouteInNewWindow(`/ko/task/${id}`), id);
      const labels = await until(() => windows(board), list => list.some(label => !before.has(label)), 'Detail window not created');
      const label = labels.find(candidate => !before.has(candidate));
      const page = board.forWindow(label);
      await page.waitForFunction(() => !!document.querySelector('[data-testid="terminal-tab-bar"]'));
      await page.evaluate(() => {
        window.__qaDialogMessages = [];
        window.__qaDialogDecision = 'Cancel';
        const original = window.fetch.bind(window);
        window.fetch = (url, options) => {
          if (decodeURIComponent(String(url)).includes('plugin:dialog|message')) {
            const payload = JSON.parse(options.body);
            window.__qaDialogMessages.push(payload.message);
            return new Promise(resolve => {
              window.__qaResolveDialog = decision => resolve(new Response(JSON.stringify(decision), {
                headers: { 'Tauri-Response': 'ok', 'Content-Type': 'application/json' },
              }));
            });
          }
          return original(url, options);
        };
      });
      return { page, label };
    }
    const waitClosed = label => until(() => windows(board), list => !list.includes(label), 'Window did not close');
    async function startPlainProcess(page, id, name) {
      const active = (await tabs(page, id)).find(tab => tab.isActive);
      const pidFile = path.join(out, `${name}.pid`);
      const opened = await page.evaluate(({ id, tabId }) => window.kanvibeDesktop.openTerminal(id, tabId, 100, 30), { id, tabId: active.id });
      assert.equal(opened.ok, true, opened.error);
      await page.evaluate(({ id, tabId, data }) => window.kanvibeDesktop.writeTerminal(id, tabId, data),
        { id, tabId: active.id, data: `echo $$ > ${quote(pidFile)}; exec sleep 600\r` });
      await until(() => fs.existsSync(pidFile), Boolean, 'PID file not written');
      const pid = Number(fs.readFileSync(pidFile, 'utf8').trim());
      assert.ok(Number.isSafeInteger(pid) && pid > 1);
      processes.push(pid);
      await until(() => invoke(page, 'terminalTabs', 'getTerminalTabCloseInfo', id, active.id), info => info.processNames?.includes('sleep'), 'Foreground process not detected');
      return { tab: active, pid };
    }
    let detail = await openDetail(plainId);
    let page = detail.page;
    await page.evaluate(() => document.querySelector('[data-testid="terminal-tab-new"]').click());
    await until(() => tabs(page, plainId), list => list.length === 2, 'Second tab missing');
    await page.waitForFunction(() => document.querySelectorAll('.xterm-helper-textarea').length > 0);
    let worker = await startPlainProcess(page, plainId, 'plain-multi');
    await page.screenshot({path:path.join(out,'before-shortcut.png')});
    await step('shortcut cancel retains tab and process', async () => {
      await closeWithDialog(page, () => key(page), false, ['sleep']);
      assert.equal((await tabs(page, plainId)).length, 2);
      assert.ok(alive(worker.pid));
      await page.screenshot({ path: path.join(out, 'plain-cancel.png') });
    });
    await step('close button cancel retains process', async () => {
      await closeWithDialog(page, () => page.evaluate(tab => document.querySelector(`[data-testid="terminal-tab-close-${tab}"]`).click(), worker.tab.id), false, ['sleep']);
      assert.equal((await tabs(page, plainId)).length, 2);
      assert.ok(alive(worker.pid));
    });
    await step('shortcut confirm removes only target tab and terminates process', async () => {
      await closeWithDialog(page, () => key(page), true, ['sleep']);
      await until(() => tabs(page, plainId), list => list.length === 1, 'Tab not removed');
      await until(() => alive(worker.pid), value => !value, 'Process still running');
    });
    worker = await startPlainProcess(page, plainId, 'plain-single');
    await step('last plain tab cancel retains process', async () => {
      await closeWithDialog(page, () => key(page), false, ['sleep']);
      assert.ok(alive(worker.pid));
    });
    await step('window close shortcut retains plain process', async () => {
      await page.evaluate(() => { window.__qaDialogMessages = []; });
      await key(page, true);
      await waitClosed(detail.label);
      assert.ok(alive(worker.pid));
    });
    detail = await openDetail(plainId); page = detail.page;
    await step('last plain tab confirmation closes window and process', async () => {
      await closeWithDialog(page, () => key(page), true, ['sleep'], true);
      await waitClosed(detail.label);
      await until(() => alive(worker.pid), value => !value, 'Last process still running');
    });
    const task = await invoke(board, 'kanban', 'createTask', { title: 'tmux close QA', projectId, baseBranch: 'main' });
    const connected = await invoke(board, 'kanban', 'connectTerminalSession', task.id, 'tmux');
    assert.equal(connected.sessionType, 'tmux');
    sessionName = connected.sessionName;
    // Background WebViews intentionally stop tab polling; start this fixture session before mounting its route.
    tmux('new-session', '-d', '-s', sessionName, '-c', fixture);
    detail = await openDetail(task.id); page = detail.page;
    const initial = (await tabs(page, task.id))[0];
    await page.evaluate(() => document.querySelector('[data-testid="terminal-tab-new"]').click());
    const list = await until(() => tabs(page, task.id), list => list.length === 2, 'tmux second tab missing');
    const windowId = list.find(tab => tab.id !== initial.id).id;
    await page.waitForFunction(id => !!document.querySelector(`[data-terminal-tab-id="${id}"][aria-selected="true"]`), windowId);
    const pidFile = path.join(out, 'tmux-worker.pid');
    const splitFile = path.join(out, 'tmux-split.pid');
    tmux('send-keys', '-t', windowId, `echo $$ > ${quote(pidFile)}; exec sleep 600`, 'C-m');
    tmux('split-window', '-t', windowId, `echo $$ > ${quote(splitFile)}; exec tail -f /dev/null`);
    await until(() => tmux('list-panes', '-t', windowId, '-F', '#{pane_current_command}'), commands => commands.includes('sleep') && commands.includes('tail'), 'tmux process names missing');
    const pids = [pidFile, splitFile].map(file => Number(fs.readFileSync(file, 'utf8').trim()));
    processes.push(...pids);
    await step('tmux split tab cancel lists both process names and retains both', async () => {
      await closeWithDialog(page, () => key(page), false, ['sleep', 'tail']);
      assert.ok(pids.every(alive));
      await page.screenshot({ path: path.join(out, 'tmux-cancel.png') });
    });
    await step('tmux split tab confirm terminates both processes', async () => {
      await closeWithDialog(page, () => key(page), true, ['sleep', 'tail']);
      await until(() => tabs(page, task.id), list => list.length === 1, 'tmux tab not removed');
      await until(() => pids.some(alive), value => !value, 'tmux processes still running');
    });
    await step('last tmux tab shortcut closes window and retains session without confirmation', async () => {
      await page.evaluate(() => { window.__qaDialogMessages = []; });
      await key(page);
      await waitClosed(detail.label);
      tmux('has-session', '-t', sessionName);
      assert.equal((await tabs(board, task.id)).length, 1);
    });
  } catch (failure) {
    error = failure.stack;
    console.error(failure);
  } finally {
    const cleanupErrors = [];
    if (sessionName && spawnSync('tmux', ['has-session', '-t', sessionName]).status === 0) {
      try { tmux('kill-session', '-t', sessionName); } catch (failure) { cleanupErrors.push(failure.message); }
    }
    if (app) { fs.writeFileSync(path.join(out, 'runtime.log'), app.output()); await app.close(); }
    for (const pid of processes.filter(alive)) {
      try { process.kill(pid, 'SIGTERM'); } catch (failure) { if (failure.code !== 'ESRCH') cleanupErrors.push(failure.message); }
    }
    await until(() => processes.filter(alive), pids => !pids.length, 'QA processes remain').catch(failure => cleanupErrors.push(failure.message));
    checks.push({ name: 'QA process cleanup', ok: cleanupErrors.length === 0, detail: cleanupErrors });
    const unchanged = fingerprint === exec('git', ['diff', 'HEAD', '--binary'], root);
    const result = { head, unchanged, checks, dialogs, error, ok: !error && checks.length === 10 && checks.every(check => check.ok) && unchanged,
      dialogStrategy: 'IPC button-result injection; actual OS dialog visibility and buttons require separate GUI verification.' };
    fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2));
    console.log(`REPORT ${path.join(out, 'result.json')}`);
    if (!result.ok) process.exitCode = 1;
  }
}
run().catch(error => { console.error(error); process.exitCode = 1; });
