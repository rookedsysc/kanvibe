const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const assert = require('node:assert/strict');
const root = process.cwd();
const load = createRequire(path.join(root, 'package.json'));
const { launchKanVibeElectron } = load('./qa/electron/lib/launchElectron.cjs');
const { expect } = load('@playwright/test');
const run = fs.mkdtempSync('/tmp/kanvibe-clipboard-qa-');
const workspace = path.join(run, 'workspace');
fs.mkdirSync(workspace);
const fileText = '# 파일 원문\n\n- 한글과 줄바꿈';
fs.writeFileSync(path.join(workspace, 'notes.md'), fileText);
const messageText = '메시지 원문\n\n```sh\necho clipboard\n```';
const checks = [];
let app, page;
let lastText;
(async () => {
  try {
    ({ app, page } = await launchKanVibeElectron({ rendererUrl: 'http://127.0.0.1:5188', appDataDir: path.join(run, 'app-data'), recordVideo: { dir: path.join(run, 'video') } }));
    await page.waitForLoadState('domcontentloaded');
    await app.evaluate(({ clipboard }) => {
      globalThis.__clipboardQaBackup = { text: clipboard.readText(), html: clipboard.readHTML(), rtf: clipboard.readRTF(), image: clipboard.readImage() };
    });
    const taskId = await app.evaluate(async (_electron, { root, workspace }) => {
      const load = process.getBuiltinModule('module').createRequire(root + '/electron/main.js');
      const db = load(root + '/src/lib/database.ts');
      const projects = await db.getProjectRepository();
      const tasks = await db.getTaskRepository();
      const project = await projects.save(projects.create({ name: 'Clipboard QA', repoPath: workspace, defaultBranch: 'main' }));
      const task = await tasks.save(tasks.create({ projectId: project.id, title: 'Clipboard QA', branchName: 'main', worktreePath: workspace, status: 'todo' }));
      return task.id;
    }, { root, workspace });
    const mount = async (kind) => page.evaluate(async ({ kind, taskId, messageText }) => {
      const componentSource = await (await fetch('/src/desktop/renderer/components/AiChatTranscriptMessage.tsx')).text();
      const mainSource = await (await fetch('/src/desktop/renderer/main.tsx')).text();
      const modulePath = (source, name) => source.match(new RegExp('from "([^"]*/' + name + '\\.js[^\"]*)"'))[1];
      const React = (await import(modulePath(componentSource, 'react'))).default;
      const { createRoot } = (await import(modulePath(mainSource, 'react-dom_client'))).default;
      const intlModule = await import(modulePath(componentSource, 'next-intl'));
      const { NextIntlClientProvider } = intlModule.default ?? intlModule;
      const messages = (await import('/messages/ko.json')).default;
      const { AiChatTranscriptMessage } = await import('/src/desktop/renderer/components/AiChatTranscriptMessage.tsx');
      const { AiChatFilePreview } = await import('/src/desktop/renderer/components/AiChatFilePreview.tsx');
      let host = document.getElementById('clipboard-qa');
      if (!host) {
        host = document.createElement('div'); host.id = 'clipboard-qa';
        host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:#12141a;color:white;padding:32px;overflow:auto';
        document.body.append(host);
        window.__clipboardQaRoot = createRoot(host);
      }
      window.__clipboardQaRoot.render(React.createElement(NextIntlClientProvider, { locale: 'ko', messages },
        kind === 'file'
          ? React.createElement(AiChatFilePreview, { taskId, filePath: 'notes.md', onClose: () => {} })
          : React.createElement(AiChatTranscriptMessage, { provider: 'codex', message: { role: 'assistant', timestamp: null, text: 'preview', fullText: messageText, isTruncated: true } })
      ));
    }, { kind, taskId, messageText });
    await mount('message');
    const host = page.locator('#clipboard-qa');
    await host.locator('.kv-chat-code').waitFor();
    const browserWrite = await page.evaluate(async () => {
      try { await navigator.clipboard.writeText('browser clipboard probe'); return 'accepted'; }
      catch (error) { return error.name + ': ' + error.message; }
    });
    checks.push({ name: 'browser API under real app permission policy', result: browserWrite });
    await host.getByRole('button', { name: '복사', exact: true }).last().click();
    await expect(host.getByRole('button', { name: '복사됨', exact: true })).toBeVisible();
    lastText = messageText;
    assert.equal(await app.evaluate(({ clipboard }) => clipboard.readText()), lastText);
    checks.push({ name: 'message button → preload → IPC → OS clipboard', status: 'passed' });
    await host.locator('.kv-chat-code').getByRole('button', { name: '복사', exact: true }).click();
    lastText = 'echo clipboard\n';
    await expect.poll(() => app.evaluate(({ clipboard }) => clipboard.readText())).toBe(lastText);
    checks.push({ name: 'code block button copies only code', status: 'passed' });
    await mount('file');
    await host.getByRole('heading', { name: '파일 원문' }).waitFor();
    await host.getByRole('button', { name: '복사', exact: true }).click();
    lastText = fileText;
    await expect.poll(() => app.evaluate(({ clipboard }) => clipboard.readText())).toBe(lastText);
    checks.push({ name: 'file preview button copies original file', status: 'passed' });
    await page.screenshot({ path: path.join(run, 'clipboard.png') });
  } catch (error) {
    checks.push({ name: 'run', status: 'failed', error: error.stack });
    if (page) await page.screenshot({ path: path.join(run, 'failure.png') }).catch(() => {});
    process.exitCode = 1;
  } finally {
    if (app) {
      await app.evaluate(({ clipboard }, lastText) => {
        const backup = globalThis.__clipboardQaBackup;
        if (backup && (clipboard.readText() === lastText || clipboard.readText() === 'browser clipboard probe')) clipboard.write(backup);
      }, lastText).catch(() => {});
      await app.close();
    }
    fs.writeFileSync(path.join(run, 'results.json'), JSON.stringify(checks, null, 2));
    console.log(JSON.stringify({ run, checks }));
  }
})();
