#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const net = require('node:net');
const { execFileSync } = require('node:child_process');
const { launchKanVibeTauri } = require('../../tauri/lib/launchTauri.cjs');

const root = path.resolve(__dirname, '../../..');
const runDirIndex = process.argv.indexOf('--run-dir');
if (runDirIndex < 0 || !process.argv[runDirIndex + 1]) throw new Error('--run-dir <fresh directory> is required');
const runDir = path.resolve(process.argv[runDirIndex + 1]);
const portIndex = process.argv.indexOf('--port');
const selectedPort = portIndex >= 0 ? Number(process.argv[portIndex + 1]) : 19738;
if (!Number.isInteger(selectedPort) || selectedPort < 1024 || selectedPort > 65535) throw new Error('--port must be1024..65535');
const labelIndex = process.argv.indexOf('--fixture-label');
const fixtureLabel = labelIndex >= 0 ? process.argv[labelIndex + 1]?.trim() : '';
if (labelIndex >= 0 && (!fixtureLabel || !/^[\p{L}\p{N}][\p{L}\p{N} _-]{0,39}$/u.test(fixtureLabel))) throw new Error('--fixture-label must contain1..40 letters, digits, spaces, underscores or hyphens');
const taskTitle = fixtureLabel ? `Chat History Fixture - ${fixtureLabel}` : 'Chat History Fixture';
const historyPrefix = fixtureLabel ? `History QA ${fixtureLabel}` : 'History QA';
const hermesTitle = `${historyPrefix} Hermes session`;
const desktopApi = `http://127.0.0.1:${selectedPort}`;
const fakeHome = path.join(runDir, 'agent-home');
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
/** 서버·런타임 전용 값이 앱과 그 자식 셸로 새지 않도록 지운 로컬 환경 */
function createLocalShellEnvironment() {
  return Object.fromEntries(Object.entries(process.env).filter(([key]) => !['PORT', 'HOST', 'NODE_ENV'].includes(key) && !key.startsWith('KANVIBE_')));
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
/** 접근성 이름(aria-label 또는 공백을 정리한 글자)이 정확히 같은 버튼을 WebView 안에서 찾는다 */
const FIND_BUTTON = `const find=(name)=>[...document.querySelectorAll('button')].find((button)=>(button.getAttribute('aria-label')||button.textContent.replace(/\\s+/g,' ').trim())===name);`;
async function waitUntil(page, description, fn, arg) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (await page.evaluate(fn, arg)) return;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Timed out waiting for ${description}`);
}
const buttonVisible = (page, name) => waitUntil(page, `button ${name}`, new Function('name', `${FIND_BUTTON}const button=find(name);return Boolean(button&&button.offsetParent);`), name);
async function clickButton(page, name) {
  await buttonVisible(page, name);
  await page.evaluate(new Function('name', `${FIND_BUTTON}find(name).click();return true;`), name);
}
const textVisible = (page, text) => waitUntil(page, `text ${text}`, (expected) => [...document.querySelectorAll('body *')].some((element) => element.children.length === 0 && element.textContent.trim() === expected && element.offsetParent), text);
/** Tauri WebView에서 데스크톱 History 목록·메시지·이전 메시지·재열기를 확인한다 */
async function runDesktopHistoryJourney(page, taskId) {
  await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('qa_resize', { width: 1440, height: 960 }));
  await page.evaluate((id) => { location.hash = `/ko/task/${id}`; return true; }, taskId);
  await page.evaluate(() => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); return true; });
  await page.evaluate(() => { const close = [...document.querySelectorAll('button')].find((button) => (button.getAttribute('aria-label') || '').includes('업데이트') && (button.getAttribute('aria-label') || '').includes('닫기')); close?.click(); return true; });
  await clickButton(page, 'AI 채팅');
  await waitUntil(page, 'ai-session-list', () => Boolean(document.querySelector('[data-testid="ai-session-list"]')));
  const latestSession = `Codex ${historyPrefix} session 24`;
  await buttonVisible(page, latestSession);
  await page.screenshot({ path: path.join(runDir, 'desktop-history-list.png') });
  await clickButton(page, '채팅 더 불러오기');
  await buttonVisible(page, `Codex ${historyPrefix} session 0`);
  await clickButton(page, latestSession);
  await textVisible(page, `${historyPrefix} message 59`);
  await page.screenshot({ path: path.join(runDir, 'desktop-history-messages.png') });
  await clickButton(page, '이전 메시지 더 보기');
  await textVisible(page, `${historyPrefix} message 1`);
  await page.screenshot({ path: path.join(runDir, 'desktop-history-older.png') });
  await clickButton(page, 'AI 채팅');
  await waitUntil(page, 'inline-ai-chat detached', () => !document.querySelector('[data-testid="inline-ai-chat"]'));
  await clickButton(page, 'AI 채팅');
  await buttonVisible(page, latestSession);
  await page.screenshot({ path: path.join(runDir, 'desktop-history-reopen.png') });
}
async function main() {
  if (fs.existsSync(path.join(runDir, 'app-data'))) throw new Error('Use a fresh --run-dir');
  fs.mkdirSync(repo, { recursive: true });
  await freePort(selectedPort);
  const git = (...args) => execFileSync('git', ['-C', repo, ...args], { env: createLocalShellEnvironment(), stdio: 'pipe' });
  git('init', '-b', 'main');
  git('config', 'user.name', 'KanVibe QA'); git('config', 'user.email', 'qa@kanvibe.local');
  fs.writeFileSync(path.join(repo, 'README.md'), '# Isolated chat History fixture\n');
  git('add', 'README.md'); git('commit', '-m', 'chore: isolated History fixture');
  seedAgentFiles();
  let app;
  const stop = async () => { if (app) await app.close().catch(() => {}); };
  process.once('SIGTERM', () => { void stop().then(() => process.exit(0)); });
  process.once('SIGINT', () => { void stop().then(() => process.exit(0)); });
  try {
    let page;
    ({ app, page } = await launchKanVibeTauri({ rootDir: root, appDataDir: path.join(runDir, 'app-data'), hookPort: selectedPort, env: isolatedAgentEnvironment(), timeoutMs: 60000 }));
    app.process().stdout?.on('data', (data) => fs.appendFileSync(path.join(runDir, 'desktop.log'), data));
    app.process().stderr?.on('data', (data) => fs.appendFileSync(path.join(runDir, 'desktop.log'), data));
    const invoke = (namespace, method, ...args) => page.evaluate(({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args), { namespace, method, args });
    await invoke('appSettings', 'setBackgroundSyncEnabled', false);
    await invoke('appSettings', 'setDefaultSessionType', 'terminal');
    const project = await invoke('project', 'registerProject', historyPrefix, repo);
    if (!project.success) throw new Error(JSON.stringify(project));
    const task = await invoke('kanban', 'createTask', { title: taskTitle, projectId: project.project.id });
    await waitFor(`${desktopApi}/api/hooks/health`);
    const pairing = await fetch(`${desktopApi}/api/mobile/dev/pairing-code`, { method: 'POST' }).then((r) => r.json());
    const paired = await fetch(`${desktopApi}/api/mobile/pair`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code: pairing.code, deviceName: 'History contract probe' }) }).then((r) => r.json());
    const response = await fetch(`${desktopApi}/api/mobile/tasks/${task.id}/ai-sessions?limit=20`, { headers: { Authorization: `Bearer ${paired.token}` } });
    const history = await response.json();
    if (response.status !== 200 || history.history.sessions.length !== 20 || !history.history.nextCursor) throw new Error(`Real History contract failed: ${JSON.stringify(history)}`);
    fs.writeFileSync(path.join(runDir, 'fixture.json'), JSON.stringify({ taskId: task.id, fixtureLabel, taskTitle, historyPrefix, hermesTitle, runtime: 'tauri', repo, fakeHome, sessionCount: history.history.sessions.length, desktopPort: selectedPort, desktopApi }, null, 2));
    fs.writeFileSync(path.join(runDir, 'http-contract.json'), JSON.stringify(history, null, 2));
    const flowDirectory = path.join(__dirname, '../../../mobile/.maestro/flows');
    const flowTemplate = fs.readFileSync(path.join(flowDirectory, 'chat-history.yaml'), 'utf8');
    const flow = flowTemplate.replace('TASK_TITLE: \"Chat History Fixture\"', `TASK_TITLE: ${JSON.stringify(taskTitle)}`).replace('HISTORY_PREFIX: \"History QA\"', `HISTORY_PREFIX: ${JSON.stringify(historyPrefix)}`).replace('- eraseText: 21', `- eraseText: ${`${historyPrefix} message 59`.length}`).replace('DESKTOP_PORT: "19738"', `DESKTOP_PORT: "${selectedPort}"`).replace('http://127.0.0.1:19738', desktopApi).replace('file: ../scripts/issue-dev-pairing-code.js', `file: ${path.resolve(flowDirectory, '../scripts/issue-dev-pairing-code.js')}`);
    fs.writeFileSync(path.join(runDir, 'chat-history.yaml'), flow);

    if (process.argv.includes('--desktop-ui')) {
      await runDesktopHistoryJourney(page, task.id);
      fs.writeFileSync(path.join(runDir, 'desktop-ui-result.json'), JSON.stringify({ pass: true, checks: ['real pairing HTTP contract', '26sessions paginated', 'selected latest messages', 'older messages', 'reopen History'] }, null, 2));
    }
    console.log(JSON.stringify({ ready: true, taskId: task.id, fixtureLabel, taskTitle, historyPrefix, runDir }));
  } catch (error) { await stop(); throw error; }
}
main().catch((error) => { console.error(error); process.exit(1); });
