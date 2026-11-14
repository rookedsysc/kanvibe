#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const net = require('node:net');
const { spawn, execFileSync } = require('node:child_process');
const { _electron: electron } = require('@playwright/test');
const { createLocalShellEnvironment } = require('../../../src/lib/shellEnvironment.ts');

const workspaceIndex = process.argv.indexOf('--workspace');
const root = workspaceIndex >= 0 ? path.resolve(process.argv[workspaceIndex + 1]) : path.resolve(__dirname, '../../..');
const runDirIndex = process.argv.indexOf('--run-dir');
if (runDirIndex < 0 || !process.argv[runDirIndex + 1]) throw new Error('--run-dir <fresh directory> is required');
const runDir = path.resolve(process.argv[runDirIndex + 1]);
const portIndex = process.argv.indexOf('--port');
const selectedPort = portIndex >= 0 ? Number(process.argv[portIndex + 1]) : 19738;
// src/lib/hookEndpoint.ts:getMobileTlsPort derives the mobile TLS listener from hook port + 10.
const MOBILE_TLS_PORT_OFFSET = 10;
const MAX_HOOK_PORT = 65535 - MOBILE_TLS_PORT_OFFSET;
if (!Number.isInteger(selectedPort) || selectedPort < 1024 || selectedPort > MAX_HOOK_PORT) throw new Error(`--port must be1024..${MAX_HOOK_PORT}`);
const mobileTlsPort = selectedPort + MOBILE_TLS_PORT_OFFSET;
const labelIndex = process.argv.indexOf('--fixture-label');
const fixtureLabel = labelIndex >= 0 ? process.argv[labelIndex + 1]?.trim() : '';
if (labelIndex >= 0 && (!fixtureLabel || !/^[\p{L}\p{N}][\p{L}\p{N} _-]{0,39}$/u.test(fixtureLabel))) throw new Error('--fixture-label must contain1..40 letters, digits, spaces, underscores or hyphens');
const taskTitle = fixtureLabel ? `Chat History Fixture - ${fixtureLabel}` : 'Chat History Fixture';
const historyPrefix = fixtureLabel ? `History QA ${fixtureLabel}` : 'History QA';
const hermesTitle = `${historyPrefix} Hermes session`;
const desktopApi = `http://127.0.0.1:${selectedPort}`;
// Herdr의 Unix socket 경로 상한을 넘기지 않는 격리된 HOME만 별도로 만든다.
const fakeHome = process.argv.includes('--short-agent-home') ? fs.realpathSync(fs.mkdtempSync('/tmp/kv-chat-')) : path.join(runDir, 'agent-home');
const repo = path.join(runDir, 'fixture-repo');

async function freePort(port = 0) {
  const server = net.createServer();
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '0.0.0.0', resolve); });
  const selected = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return selected;
}
async function waitFor(url) {
  for (let i = 0; i < 300; i++) {
    try { if ((await fetch(url, { signal: AbortSignal.timeout(1000) })).ok) return; } catch {}
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Fixture did not become ready: ${url}`);
}
function isolatedAgentEnvironment() {
  const environment = createLocalShellEnvironment();
  for (const key of ['CODEX_HOME', 'CLAUDE_CONFIG_DIR', 'HERMES_HOME', 'PI_CODING_AGENT_DIR', 'OMP_CODING_AGENT_DIR', 'PRIME_AGENT_SESSION_DIR', 'PRIME_AGENT_CODING_AGENT_SESSION_DIR', 'PRIME_AGENT_CODING_AGENT_DIR', 'COPILOT_HOME', 'GROK_HOME', 'DEVIN_HOME', 'CLINE_SESSION_DATA_DIR', 'OPENCLAW_STATE_DIR', 'KIMI_CODE_HOME', 'JCODE_HOME', 'KIRO_HOME']) delete environment[key];
  return { ...environment, HOME: fakeHome, XDG_DATA_HOME: path.join(fakeHome, '.local', 'share'), XDG_CONFIG_HOME: path.join(fakeHome, '.config'), XDG_CACHE_HOME: path.join(fakeHome, '.cache') };
}
function seedAgentFiles() {
  const sessions = path.join(fakeHome, '.codex', 'sessions');
  fs.mkdirSync(sessions, { recursive: true });
  for (let i = 0; i < 25; i++) {
    const id = `qa-session-${i}`;
    const events = [{ type: 'session_meta', payload: { id, cwd: repo } }];
    for (let message = 0; message < 60; message++) events.push({
      type: 'response_item', timestamp: new Date(Date.UTC(2026, 10, 2, 0, i, message)).toISOString(),
      payload: { type: 'message', role: message % 2 ? 'assistant' : 'user', content: [{ type: 'text', text: message === 0 ? `${historyPrefix} session ${i}` : `${historyPrefix} message ${message}` }] },
    });
    fs.writeFileSync(path.join(sessions, `${id}.jsonl`), events.map((entry) => JSON.stringify(entry)).join('\n') + '\n');
  }
  const hermesDirectory = path.join(fakeHome, '.hermes', 'sessions');
  fs.mkdirSync(hermesDirectory, { recursive: true });
  fs.writeFileSync(path.join(hermesDirectory, 'session_qa-hermes.json'), JSON.stringify({
    session_id: 'qa-hermes', cwd: repo, session_start: '2026-11-02T00:25:00.000Z', last_updated: '2026-11-02T00:25:01.000Z',
    messages: [
      { role: 'user', content: hermesTitle, timestamp: '2026-11-02T00:25:00.000Z' },
      { role: 'assistant', content: `${historyPrefix} Hermes reply`, timestamp: '2026-11-02T00:25:01.000Z' },
    ],
  }));
}
async function main() {
  if (fs.existsSync(path.join(runDir, 'app-data'))) throw new Error('Use a fresh --run-dir');
  fs.mkdirSync(repo, { recursive: true });
  // 격리 HOME에 zsh 시작 파일이 없으면 zsh-newuser-install 화면이 떠서 fixture가 터미널에 친 입력을 먹는다
  fs.mkdirSync(fakeHome, { recursive: true });
  fs.writeFileSync(path.join(fakeHome, '.zshrc'), '');
  await freePort(selectedPort);
  await freePort(mobileTlsPort);
  const git = (...args) => execFileSync('git', ['-C', repo, ...args], { env: createLocalShellEnvironment(), stdio: 'pipe' });
  git('init', '-b', 'main');
  git('config', 'user.name', 'KanVibe QA'); git('config', 'user.email', 'qa@kanvibe.local');
  fs.writeFileSync(path.join(repo, 'README.md'), '# Isolated chat History fixture\n');
  git('add', 'README.md'); git('commit', '-m', 'chore: isolated History fixture');
  seedAgentFiles();
  const rendererPort = await freePort();
  const cdpPort = await freePort();
  const rendererUrl = `http://127.0.0.1:${rendererPort}`;
  const viteLog = fs.openSync(path.join(runDir, 'vite.log'), 'a');
  const vite = spawn(process.execPath, [path.join(root, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', String(rendererPort)], { cwd: root, env: createLocalShellEnvironment(), stdio: ['ignore', viteLog, viteLog] });
  let app;
  const stop = async () => { if (app) await app.close().catch(() => {}); vite.kill('SIGTERM'); };
  process.once('SIGTERM', () => { void stop().then(() => process.exit(0)); });
  process.once('SIGINT', () => { void stop().then(() => process.exit(0)); });
  try {
    await waitFor(rendererUrl);
    app = await electron.launch({ executablePath: require('electron'), args: [`--remote-debugging-port=${cdpPort}`, '--no-sandbox', `--kanvibe-hook-port=${selectedPort}`, root], cwd: root, env: { ...isolatedAgentEnvironment(), KANVIBE_QA_MODE: '1', KANVIBE_APP_DATA_DIR: path.join(runDir, 'app-data'), KANVIBE_RENDERER_URL: rendererUrl }, recordVideo: process.argv.includes('--pane-chat') ? { dir: path.join(runDir, 'video') } : undefined, timeout: 60000 });
    app.process().stdout?.on('data', (data) => fs.appendFileSync(path.join(runDir, 'desktop.log'), data));
    app.process().stderr?.on('data', (data) => fs.appendFileSync(path.join(runDir, 'desktop.log'), data));
    const page = await app.firstWindow();
    await page.waitForFunction(() => Boolean(window.kanvibeDesktop?.invoke));
    const invoke = (namespace, method, ...args) => page.evaluate(({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args), { namespace, method, args });
    await invoke('appSettings', 'setBackgroundSyncEnabled', false);
    await invoke('appSettings', 'setDefaultSessionType', 'terminal');
    const project = await invoke('project', 'registerProject', historyPrefix, repo);
    if (!project.success) throw new Error(JSON.stringify(project));
    const task = await invoke('kanban', 'createTask', { title: taskTitle, projectId: project.project.id, ...(process.argv.includes('--pane-chat') ? { baseBranch: 'main' } : {}) });
    if (process.argv.includes('--pane-chat')) {
      const connected = await invoke('kanban', 'connectTerminalSession', task.id, 'terminal');
      if (connected?.sessionType !== 'terminal' || !connected.sessionName) throw new Error('Fixture terminal session did not connect');
    }
    await waitFor(`${desktopApi}/api/hooks/health`);
    const pairing = await fetch(`${desktopApi}/api/mobile/dev/pairing-code`, { method: 'POST' }).then((r) => r.json());
    const paired = await fetch(`${desktopApi}/api/mobile/pair`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code: pairing.code, deviceName: 'History contract probe' }) }).then((r) => r.json());
    const response = await fetch(`${desktopApi}/api/mobile/tasks/${task.id}/ai-sessions?limit=20`, { headers: { Authorization: `Bearer ${paired.token}` } });
    const history = await response.json();
    if (response.status !== 200 || history.history.sessions.length !== 20 || !history.history.nextCursor) throw new Error(`Real History contract failed: ${JSON.stringify(history)}`);
    fs.writeFileSync(path.join(runDir, 'fixture.json'), JSON.stringify({ taskId: task.id, fixtureLabel, taskTitle, historyPrefix, hermesTitle, rendererUrl, repo, fakeHome, sessionCount: history.history.sessions.length, desktopPort: selectedPort, mobileTlsPort, desktopApi, cdpPort }, null, 2));
    fs.writeFileSync(path.join(runDir, 'http-contract.json'), JSON.stringify(history, null, 2));
    const flowDirectory = path.join(__dirname, '../../../mobile/.maestro/flows');
    const flowTemplate = fs.readFileSync(path.join(flowDirectory, 'chat-history.yaml'), 'utf8');
    const flow = flowTemplate.replace('TASK_TITLE: \"Chat History Fixture\"', `TASK_TITLE: ${JSON.stringify(taskTitle)}`).replace('HISTORY_PREFIX: \"History QA\"', `HISTORY_PREFIX: ${JSON.stringify(historyPrefix)}`).replace('DESKTOP_PORT: "19748"', `DESKTOP_PORT: "${mobileTlsPort}"`).replace('http://127.0.0.1:19738', desktopApi).replace('file: ../scripts/issue-dev-pairing-code.js', `file: ${path.resolve(flowDirectory, '../scripts/issue-dev-pairing-code.js')}`);
    fs.writeFileSync(path.join(runDir, 'chat-history.yaml'), flow);

    if (process.argv.includes('--pane-chat')) {
      const { runPaneChatScenario } = require('../../electron/lib/paneChatScenario.cjs');
      const live = await runPaneChatScenario({ page, invoke, taskId: task.id, repo, fakeHome, runDir });
      const fixture = JSON.parse(fs.readFileSync(path.join(runDir, 'fixture.json'), 'utf8'));
      fs.writeFileSync(path.join(runDir, 'fixture.json'), JSON.stringify({ ...fixture, live }, null, 2));
    }

    if (process.argv.includes('--desktop-ui')) {
      await page.setViewportSize({ width: 1440, height: 960 });
      const navigation = await page.evaluate(async (taskId) => {
        const { navigateToTaskDetail } = await import('/src/desktop/renderer/utils/taskNavigation.ts');
        await navigateToTaskDetail(taskId, { currentLocale: 'ko' });
        return true;
      }, task.id);
      if (!navigation) throw new Error('Task navigation failed');
      await page.keyboard.press('Escape');
      const closeUpdate = page.locator("button[aria-label*='업데이트'][aria-label*='닫기']").first();
      if (await closeUpdate.isVisible().catch(() => false)) await closeUpdate.click();
      await page.getByRole('button', { name: 'AI 채팅', exact: true }).click();
      const list = page.getByTestId('ai-session-list');
      await list.waitFor({ state: 'visible', timeout: 30000 });
      const first = page.getByRole('button', { name: `Codex ${historyPrefix} session 24`, exact: true });
      await first.waitFor({ state: 'visible' });
      await page.screenshot({ path: path.join(runDir, 'desktop-history-list.png') });
      await page.getByRole('button', { name: '채팅 더 불러오기', exact: true }).click();
      await page.getByRole('button', { name: `Codex ${historyPrefix} session 0`, exact: true }).waitFor({ state: 'visible' });
      await first.click();
      await page.getByText(`${historyPrefix} message 59`, { exact: true }).waitFor({ state: 'visible' });
      await page.screenshot({ path: path.join(runDir, 'desktop-history-messages.png') });
      await page.getByRole('button', { name: '이전 메시지 더 보기', exact: true }).click();
      await page.getByText(`${historyPrefix} message 1`, { exact: true }).waitFor({ state: 'visible' });
      await page.screenshot({ path: path.join(runDir, 'desktop-history-older.png') });
      await page.getByRole('button', { name: 'AI 채팅', exact: true }).click();
      await page.getByTestId('inline-ai-chat').waitFor({ state: 'detached' });
      await page.getByRole('button', { name: 'AI 채팅', exact: true }).click();
      await first.waitFor({ state: 'visible' });
      await page.screenshot({ path: path.join(runDir, 'desktop-history-reopen.png') });
      fs.writeFileSync(path.join(runDir, 'desktop-ui-result.json'), JSON.stringify({ pass: true, checks: ['real pairing HTTP contract', '26sessions paginated', 'selected latest messages', 'older messages', 'reopen History'] }, null, 2));
    }
    console.log(JSON.stringify({ ready: true, taskId: task.id, fixtureLabel, taskTitle, historyPrefix, runDir }));
  } catch (error) {
    if (app) {
      const window = await app.firstWindow().catch(() => null);
      if (window) {
        await window.screenshot({ path: path.join(runDir, 'failure.png'), animations: 'disabled' }).catch(() => {});
        fs.writeFileSync(path.join(runDir, 'failure-page.txt'), await window.locator('body').innerText().catch(() => ''));
      }
    }
    await stop();
    throw error;
  }
}
main().catch((error) => { console.error(error); process.exit(1); });
