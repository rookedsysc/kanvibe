/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { _electron: electron } = require('@playwright/test');
const { resolveCdpPort, resolveHookServerPort } = require('../lib/launchElectron.cjs');

async function main() {
  const root = process.cwd();
  const run = path.resolve(process.argv[2] || 'qa-results/roky-harness/desktop-pane-chat-20261114/electron');
  const home = path.join(run, 'home');
  const repo = path.join(run, 'workspace');
  const session = `kv-pane-qa-${process.pid}`;
  fs.mkdirSync(repo, { recursive: true });
  fs.mkdirSync(home, { recursive: true });
  execFileSync('git', ['init', '-q', '-b', 'main', repo]);
  const tmux = (...args) => execFileSync('/opt/homebrew/bin/tmux', args, { encoding: 'utf8' }).trim();
  const quote = text => "'" + text.replace(/'/g, "'\"'\"'") + "'";
  const checks = [];
  const check = (name, detail) => checks.push({ name, detail, status: 'passed' });
  let app, page;
  let ownedSession = false;
  let error;
  const cli = path.join(run, 'codex');
  fs.copyFileSync(path.join(root, 'qa/electron/lib/pane-chat-cli.py'), cli);
  fs.writeFileSync(path.join(repo, 'notes.md'), '# 연결 흐름\n\n| 단계 | 결과 |\n| --- | --- |\n| CLI | 유지 |\n\nInline $E=mc^2$ and \\[\\frac{a}{b}\\].\n');
  fs.writeFileSync(path.join(repo, 'model.tex'), String.raw`\documentclass{article}\begin{document}\begin{equation}T_{total}=T_{queue}+T_{lookup}\end{equation}\end{document}`);
  fs.writeFileSync(path.join(repo, 'preview.html'), `<!doctype html><html><body><h1>동일 pane의 결과물</h1><p id="proof">읽는 중</p><button onclick="document.querySelector('#proof').textContent='실제 HTML 스크립트 실행'">실행</button><script>try{parent.document.body.dataset.leaked='yes'}catch(e){document.body.dataset.parentBlocked='yes'}document.body.dataset.bridge=typeof window.kanvibeDesktop;document.body.dataset.node=typeof require;</script><button id="navigate" onclick="location.href='https://example.com/should-never-leave?secret=fixture'">외부 이동 시도</button></body></html>`);
  try {
    tmux('new-session', '-d', '-s', session, '-x', '140', '-y', '42', '-c', repo);
    ownedSession = true;
    tmux('split-window', '-h', '-t', session, '-c', repo);
    const paneIds = tmux('list-panes', '-t', session, '-F', '#{pane_id}').split('\n');
    for (const [index, id] of paneIds.entries()) {
      tmux('send-keys', '-t', id, `env HOME=${quote(home)} /usr/bin/python3 ${quote(cli)} codex fixture-${index}`, 'Enter');
    }
    tmux('select-pane', '-t', paneIds[0]);
    const childEnv = { ...process.env, HOME: home, KANVIBE_QA_MODE: '1', KANVIBE_APP_DATA_DIR: path.join(run, 'app-data'), KANVIBE_DB_PATH: path.join(run, 'app-data/kanvibe.db') };
    for (const key of Object.keys(childEnv)) if (key.startsWith('HERDR_') || key.startsWith('CODEX_') || key === 'TMUX' || key === 'TMUX_PANE') delete childEnv[key];
    app = await electron.launch({ executablePath: require('electron'), cwd: root, args: [`--remote-debugging-port=${await resolveCdpPort()}`, `--kanvibe-hook-port=${await resolveHookServerPort()}`, '--no-sandbox', root], env: childEnv, recordVideo: { dir: path.join(run, 'video'), size: { width: 1440, height: 960 } }, timeout: 45000 });
    page = await app.firstWindow();
    page.setDefaultTimeout(15000);
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.waitForLoadState('domcontentloaded');
    const task = await app.evaluate(async (_electron, { root, repo, session }) => {
      const load = process.getBuiltinModule('module').createRequire(root + '/electron/main.js');
      const db = load(root + '/build/main/src/lib/database.js');
      const tasks = await db.getTaskRepository();
      const projects = await db.getProjectRepository();
      const project = await projects.save(projects.create({ name: 'KanVibe QA', repoPath: repo, defaultBranch: 'main' }));
      return tasks.save(tasks.create({ projectId: project.id, branchName: 'main', baseBranch: 'main', title: 'Pane chat QA · 같은 CLI 유지', worktreePath: repo, sessionType: 'tmux', sessionName: session, status: 'progress' }));
    }, { root, repo, session });
    const invoke = (method, ...args) => page.evaluate(({ method, args }) => window.kanvibeDesktop.invoke('paneChat', method, args), { method, args });
    await page.evaluate(taskId => { location.hash = '#/ko/task/' + taskId; }, task.id);
    await page.locator('.xterm').first().waitFor({ state: 'visible', timeout: 30000 });
    await page.keyboard.press('Escape');
    const until = async (read, predicate, label) => {
      const deadline = Date.now() + 30000;
      let result;
      while (Date.now() < deadline) {
        result = await read();
        if (predicate(result)) return result;
        await page.waitForTimeout(200);
      }
      throw Error(label + ': ' + JSON.stringify(result));
    };
    const initial = await until(() => invoke('getPaneChatSnapshot', task.id, paneIds[0]), s => s.canSend && s.transcript.sessionId === 'fixture-0', 'exact A transcript');
    const second = await until(() => invoke('getPaneChatSnapshot', task.id, paneIds[1]), s => s.canSend && s.transcript.sessionId === 'fixture-1', 'exact B transcript');
    assert.notEqual(initial.targetToken, second.targetToken);
    const processInfo = () => tmux('list-panes', '-t', session, '-F', '#{pane_id}:#{pane_pid}:#{pane_left}:#{pane_top}:#{pane_width}:#{pane_height}');
    const processStart = processInfo();
    const pidsBefore = execFileSync('ps', ['-wwaxo', 'pid=,ppid=,lstart=,args='], { encoding: 'utf8' }).split('\n').filter(line => line.includes(cli));
    assert.equal(pidsBefore.length, 2);
    const outside = tmux('capture-pane', '-p', '-t', paneIds[1]);
    await page.getByTestId('pane-chat-toggle').click();
    await page.getByTestId('pane-chat').waitFor({ state: 'visible' });
    const panel = page.locator(`[data-pane-chat-id="${paneIds[0]}"]`);
    const bounds = await panel.boundingBox();
    assert.ok(bounds && bounds.width < 900 && bounds.width > 300);
    assert.equal(await page.getByRole('button', { name: 'CLI 조작', exact: true }).count(), 0);
    const input = page.getByRole('textbox', { name: '현재 CLI에 메시지 보내기…' });
    await input.fill('Only A: $E=mc^2$');
    await page.getByRole('button', { name: '보내기', exact: true }).click();
    await until(() => invoke('getPaneChatSnapshot', task.id, paneIds[0]), s => s.transcript.messages.some(m => m.text.includes('fixture-0 received')), 'real PTY response');
    await page.locator('math').first().waitFor({ state: 'visible' });
    assert.equal(tmux('capture-pane', '-p', '-t', paneIds[1]), outside);
    assert.equal(processInfo(), processStart);
    check('two same-cwd CLI fixtures remain separate', { paneIds, sessionA: initial.transcript.sessionId, sessionB: second.transcript.sessionId, bounds, processStart, pidsBefore });
    await input.fill('Preserved pane A draft');
    await page.screenshot({ path: path.join(run, 'chat.png') });
    await page.getByRole('button', { name: '터미널로 전환', exact: true }).first().click();
    await page.getByTestId('pane-chat-toggle').click();
    await until(() => input.inputValue(), value => value === 'Preserved pane A draft', 'draft restoration');
    check('in-place switching preserves draft, CLI and sibling', { geometry: processInfo() });
    // 실제 provider 파일 읽기 경계를 거치도록 열린 JSONL에 artifact tool record를 추가한다.
    const transcript = path.join(home, '.codex/sessions/rollout-fixture-0.jsonl');
    for (const file of ['notes.md', 'model.tex', 'preview.html']) {
      fs.appendFileSync(transcript, JSON.stringify({ timestamp: new Date().toISOString(), type: 'response_item', payload: { type: 'custom_tool_call', name: 'apply_patch', input: `*** Begin Patch\n*** Add File: ${file}\n+fixture\n*** End Patch` } }) + '\n');
    }
    for (const [file, shot] of [['notes.md', 'markdown'], ['model.tex', 'latex'], ['preview.html', 'html']]) {
      const card = page.getByRole('button').filter({ hasText: file }).first();
      await card.waitFor({ state: 'visible', timeout: 30000 });
      await card.click();
      const preview = page.getByTestId('ai-chat-file-preview');
      await preview.waitFor({ state: 'visible' });
      assert.deepEqual(await panel.boundingBox(), bounds);
      if (file === 'preview.html') {
        const frame = page.frameLocator('[data-testid="ai-chat-file-document"]');
        await frame.getByRole('button', { name: '실행', exact: true }).click();
        await frame.getByText('실제 HTML 스크립트 실행', { exact: true }).waitFor();
        const proof = await page.locator('[data-testid="ai-chat-file-document"]').evaluate(async el => {
          try { return { accessible: !!el.contentDocument }; } catch { return { accessible: false }; }
        });
        assert.equal(proof.accessible, false);
        assert.equal(await frame.locator('body').getAttribute('data-parent-blocked'), 'yes');
        assert.equal(await frame.locator('body').getAttribute('data-bridge'), 'undefined');
        assert.equal(await frame.locator('body').getAttribute('data-node'), 'undefined');
        const navigations = [];
        page.on('request', request => { if (request.url().includes('should-never-leave')) navigations.push(request.url()); });
        await frame.getByRole('button', { name: '외부 이동 시도', exact: true }).click();
        await page.waitForTimeout(400);
        assert.equal(navigations.length, 0);
        assert.equal(await page.locator('body').getAttribute('data-leaked'), null);
        check('HTML opaque origin, parent access and remote navigation blocked', proof);
      } else await preview.locator('math').first().waitFor({ state: 'visible' });
      await page.screenshot({ path: path.join(run, shot + '.png') });
      await preview.getByRole('button', { name: '원문', exact: true }).click();
      await page.getByTestId('ai-chat-file-source').waitFor();
      await preview.getByRole('button', { name: '미리보기 닫기', exact: true }).click();
      check(file + ' actual preview and source', 'same owning pane');
    }
    fs.renameSync(path.join(repo, 'notes.md'), path.join(repo, 'notes.saved'));
    await page.getByRole('button').filter({ hasText: 'notes.md' }).first().click();
    await page.getByTestId('ai-chat-file-preview').getByRole('alert').waitFor();
    fs.renameSync(path.join(repo, 'notes.saved'), path.join(repo, 'notes.md'));
    await page.getByRole('button', { name: '다시 시도', exact: true }).click();
    await page.getByTestId('ai-chat-file-preview').locator('math').first().waitFor({ state: 'visible' });
    await page.getByRole('button', { name: '미리보기 닫기', exact: true }).click();
    check('actual file error and retry recover without changing panes', 'not-found → restore → retry → formula');
    await page.setViewportSize({ width: 900, height: 700 });
    await until(() => panel.boundingBox(), box => box && box.width < 600, 'responsive pane layout');
    await page.screenshot({ path: path.join(run, 'narrow.png') });
    check('responsive pane', { viewport: '900x700' });
    fs.renameSync(transcript, transcript + '.paused');
    await until(() => invoke('getPaneChatSnapshot', task.id, paneIds[0]), snapshot => !snapshot.canSend && !snapshot.targetToken, 'unavailable refuses input');
    await page.getByText('현재 CLI의 대화를 확인할 수 없습니다.', { exact: true }).waitFor();
    await page.screenshot({ path: path.join(run, 'unavailable.png') });
    await page.waitForTimeout(3500);
    fs.renameSync(transcript + '.paused', transcript);
    await page.getByRole('button', { name: '다시 연결', exact: true }).click();
    await until(() => input.inputValue(), value => value === 'Preserved pane A draft', 'conversation draft after lease renewal');
    const renewed = await invoke('getPaneChatSnapshot', task.id, paneIds[0]);
    assert.notEqual(renewed.targetToken, initial.targetToken);
    assert.equal(renewed.conversationKey, initial.conversationKey);
    check('unavailable is actionable and renewed lease restores the conversation draft', 'no transcript → disabled input → explicit retry → same draft, fresh token');
    await page.getByRole('button', { name: '모델 선택', exact: true }).click();
    await until(() => invoke('getPaneChatSnapshot', task.id, paneIds[0]), snapshot => snapshot.screen.includes('Select model'), 'native model dialog');
    await page.getByTestId('pane-chat-toggle').waitFor({ state: 'visible' });
    assert.equal(await page.getByTestId('pane-chat-controls').count(), 0);
    tmux('send-keys', '-t', paneIds[0], 'Escape');
    await until(() => invoke('getPaneChatSnapshot', task.id, paneIds[0]), snapshot => snapshot.inputState === 'ready', 'native model dialog dismissed');
    await page.getByTestId('pane-chat-toggle').click();
    await until(() => input.inputValue(), value => value === 'Preserved pane A draft', 'draft after native model dialog');
    check('model button opens verified native picker in the same terminal', 'no generic UI keys; same process; draft restored');
    assert.equal(tmux('capture-pane', '-p', '-t', paneIds[1]).includes('Only A'), false);
    const pidsAfter = execFileSync('ps', ['-wwaxo', 'pid=,ppid=,lstart=,args='], { encoding: 'utf8' }).split('\n').filter(line => line.includes(cli));
    assert.equal(pidsAfter.length, 2);
    assert.deepEqual(pidsAfter, pidsBefore);
    check('CLI execution identity never restarted', pidsAfter);
  } catch (caught) {
    error = caught;
    if (page) await page.screenshot({ path: path.join(run, 'failure.png') }).catch(() => {});
  } finally {
    if (app) await app.close();
    if (ownedSession) tmux('kill-session', '-t', session);
    fs.writeFileSync(path.join(run, 'result.json'), JSON.stringify({ status: error ? 'failed' : 'passed', provider: 'deterministic Python CLI fixtures in real PTYs; no live provider inference', checks, error: error?.stack || null }, null, 2) + '\n');
  }
  if (error) throw error;
  console.log(JSON.stringify({ status: 'passed', checks: checks.length, run }));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
