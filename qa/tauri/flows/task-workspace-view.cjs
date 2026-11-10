const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { launchKanVibeTauri } = require('../lib/launchTauri.cjs');

async function main() {
  const out = path.resolve(process.argv[2] || `qa-results/workspace-${Date.now()}`);
  if (fs.existsSync(path.join(out, 'app-data'))) throw new Error('Use a fresh QA directory');
  fs.mkdirSync(out, { recursive: true });
  const repo = path.join(out, `repository-${Date.now()}`);
  fs.mkdirSync(repo);
  const git = (...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8' });
  git('init', '-b', 'main'); git('config', 'user.name', 'QA'); git('config', 'user.email', 'qa@kanvibe.local');
  fs.writeFileSync(path.join(repo, 'README.md'), '# Workspace QA\n');
  git('add', '.'); git('commit', '-m', 'Fixture');
  const appDataDir = path.join(out, 'app-data');
  let { app, page } = await launchKanVibeTauri({ appDataDir, hookPort: 19750 });
  const checks = [];
  const invoke = (ns, method, ...args) => page.evaluate(({ ns, method, args }) => window.kanvibeDesktop.invoke(ns, method, args), { ns, method, args });
  const record = name => { checks.push({ name, ok: true }); console.log(`PASS ${name}`); };
  const palette = async command => {
    await page.evaluate(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', code: 'KeyP', metaKey: true, shiftKey: true, bubbles: true, cancelable: true })));
    await page.waitForFunction(() => !!document.querySelector('[role="dialog"] input'));
    await page.evaluate(command => {
      const input = document.querySelector('[role="dialog"] input');
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, command);
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }, command);
    await page.waitForFunction(command => [...document.querySelectorAll('[role="dialog"] button')].some(b => b.textContent.includes(command)), command);
    await page.evaluate(command => [...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent.includes(command)).click(), command);
  };
  try {
    await invoke('appSettings', 'setBackgroundSyncEnabled', false);
    await invoke('appSettings', 'setDefaultSessionType', 'terminal');
    const registered = await invoke('project', 'registerProject', 'Workspace fixture', repo);
    assert.equal(registered.success, true);
    const tasks = [];
    for (const title of ['alpha-login', 'beta-billing']) tasks.push(await invoke('kanban', 'createTask', { title, baseBranch: 'main', projectId: registered.project.id }));
    await page.evaluate(() => { location.hash = '/ko'; });
    await page.waitForFunction(() => !!document.querySelector('[data-kanban-task-card="true"]'));
    await palette('view workspace');
    await page.waitForFunction(() => !!document.querySelector('[data-testid="workspace-task-title"]'));
    for (const task of tasks) assert.equal((await invoke('kanban', 'getTaskById', task.id)).sessionName, null);
    record('palette switches view without creating an unselected session');
    const selectTask = async title => {
      await page.evaluate(title => [...document.querySelectorAll('ul button')].find(b => b.textContent.includes(title)).click(), title);
      await page.waitForFunction(title => !!document.querySelector(`section[aria-label="${title}"] .xterm`), title);
    };
    await selectTask(tasks[0].title);
    const first = await invoke('kanban', 'getTaskById', tasks[0].id);
    assert.equal(first.sessionType, 'terminal'); assert.ok(first.sessionName);
    record('selected task automatically connects a native terminal');
    await page.evaluate(() => {
      const input = document.querySelector('input[type="search"]');
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, 'beta-billing');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="workspace-task-title"]').length === 1);
    assert.equal(await page.evaluate(() => document.querySelector('[data-testid="workspace-task-title"]').textContent), 'beta-billing');
    await selectTask(tasks[1].title);
    record('search filters tasks and selection switches terminal ownership');
    await page.screenshot({ path: path.join(out, 'workspace.png') });
    assert.equal(await invoke('appSettings', 'getAppSetting', 'board_view_mode'), 'workspace');
    await app.close();
    ({ app, page } = await launchKanVibeTauri({ appDataDir, hookPort: 19750 }));
    await page.waitForFunction(() => !!document.querySelector('[data-testid="workspace-task-title"]'));
    record('workspace preference survives native application restart');
    await palette('view kanban');
    await page.waitForFunction(() => !!document.querySelector('[data-kanban-task-card="true"]'));
    assert.equal(await invoke('appSettings', 'getAppSetting', 'board_view_mode'), 'kanban');
    record('palette returns to kanban and persists preference');
    for (const task of tasks) await invoke('kanban', 'deleteTask', task.id);
    fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify({ ok: true, checks }, null, 2));
  } catch (error) {
    fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify({ ok: false, checks, error: error.stack }, null, 2));
    throw error;
  } finally {
    fs.writeFileSync(path.join(out, 'runtime.log'), app.output());
    await app.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
