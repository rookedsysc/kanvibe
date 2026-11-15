const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const root = process.cwd();
const load = createRequire(path.join(root, 'package.json'));
const { launchKanVibeElectron } = load('./qa/electron/lib/launchElectron.cjs');
const { expect } = load('@playwright/test');
const run = fs.mkdtempSync('/tmp/kanvibe-chat-feature-qa-');
const checks = [];
const modelLabel = load('./messages/ko.json').taskDetail.paneChat.model;
let app, page;
(async () => {
 try {
  ({ app, page } = await launchKanVibeElectron({ rendererUrl: 'http://127.0.0.1:5188', appDataDir: path.join(run, 'app-data'), recordVideo: { dir: path.join(run, 'video'), size: { width: 1100, height: 820 } }, viewport: { width: 1100, height: 820 } }));
  await page.waitForLoadState('domcontentloaded');
  await app.evaluate(async (_electron, root) => {
   const load = process.getBuiltinModule('module').createRequire(root + '/electron/main.js');
   const { desktopServices } = load(root + '/src/desktop/main/serviceRegistry.ts');
   const message = (role, text) => ({ role, text, fullText: text, timestamp: null, isTruncated: false });
   const settings = { model: 'qa-model-a', effort: 'high', isFast: false, models: [{ id: 'qa-model-a', label: 'QA model A', effortLevels: [{ id: 'high', label: 'High' }, { id: 'low', label: 'Low' }], supportsFast: false }, { id: 'qa-model-b', label: 'QA model B', effortLevels: [{ id: 'high', label: 'High' }], supportsFast: false }] };
   const snapshot = { taskId: 'qa-task', paneId: 'qa-pane', sessionType: 'terminal', provider: 'codex', targetToken: 'qa-lease', conversationKey: 'qa-conversation', canSend: true, inputState: 'occupied', screen: '', reason: null, modelPicker: { supported: true, mode: 'native-cli', supportsApi: true }, composer: { attachments: false, skills: false, fileMentions: false, canSendWhileBusy: true, maxAttachmentBytes: 100, maxAttachments: 0 }, modelSettings: settings, transcript: { status: 'available', reason: null, sessionId: 'qa-session', nextCursor: '100', messages: [message('assistant', '현재 대화 응답'), message('developer', '자동 주입 지침 QA')] } };
   globalThis.__chatQa = { snapshot, settings, sends: [], changes: [], release: null };
   desktopServices.paneChat = { ...desktopServices.paneChat,
    getPaneChatLayout: async () => ({ activePaneId: 'qa-pane', panes: [{ id: 'qa-pane', tabId: 'qa-tab', bounds: null }] }),
    getPaneChatSnapshot: async () => snapshot,
    getPaneChatTranscriptPage: async () => ({ ...snapshot.transcript, nextCursor: null, messages: [message('user', '이전 대화 요청 QA')] }),
    getPaneChatModelSettings: async () => settings,
    changePaneChatModelSetting: async (_task, _token, change) => { globalThis.__chatQa.changes.push(change); if (change.kind === 'model') settings.model = change.model; else if (change.kind === 'effort') settings.effort = change.effort; return { success: true }; },
    sendPaneChatPrompt: async (_task, _token, text) => {
     globalThis.__chatQa.sends.push(text);
     await new Promise((resolve) => { globalThis.__chatQa.release = resolve; });
     snapshot.transcript.messages.unshift({ ...message('user', text), timestamp: new Date().toISOString() });
     return { success: true };
    },
    focusPaneChatTerminal: async () => ({ success: true }),
   };
  }, root);
  const mount = async (kind) => page.evaluate(async (kind) => {
   const source = await (await fetch('/src/desktop/renderer/components/PaneChatView.tsx')).text();
   const main = await (await fetch('/src/desktop/renderer/main.tsx')).text();
   const modulePath = (source, name) => source.match(new RegExp('from "([^"]*/' + name + '\\.js[^\"]*)"'))[1];
   const React = (await import(modulePath(source, 'react'))).default;
   const { createRoot } = (await import(modulePath(main, 'react-dom_client'))).default;
   const intl = await import(modulePath(source, 'next-intl'));
   const { NextIntlClientProvider } = intl.default ?? intl;
   const messages = (await import('/messages/ko.json')).default;
   const { PaneChatView } = await import('/src/desktop/renderer/components/PaneChatView.tsx');
   const { PaneChatSettings } = await import('/src/desktop/renderer/components/PaneChatSettings.tsx');
   const { TerminalPaneChatLayer } = await import('/src/desktop/renderer/components/TerminalPaneChatLayer.tsx');
   const { AiChatTranscriptMessage } = await import('/src/desktop/renderer/components/AiChatTranscriptMessage.tsx');
   let host = document.getElementById('chat-feature-qa');
   if (!host) { host = document.createElement('div'); host.id = 'chat-feature-qa'; host.style.cssText = 'position:fixed;inset:0;z-index:99999;display:flex;flex-direction:column;background:#101116;color:white;overflow:hidden'; document.body.append(host); window.__chatQaRoot = createRoot(host); }
   const props = { taskId: 'qa-task', paneId: 'qa-pane', taskTitle: '채팅 동작 QA', onHistory: () => {}, onTerminal: () => { window.__leftChat = true; } };
   let node;
   if (kind === 'settings') node = React.createElement(PaneChatSettings);
   else if (kind === 'layer') node = React.createElement(TerminalPaneChatLayer, { taskId: 'qa-task', terminalRef: React.createRef(), metrics: null, isVisible: true, onHistory: () => {}, onChatFocusChange: () => {} });
   else if (kind === 'tool') node = React.createElement('div', { id: 'tool-scroll', style: { height: 350, overflowY: 'auto', margin: 30 } }, React.createElement(AiChatTranscriptMessage, { provider: 'codex', message: { role: 'tool', toolName: 'functions.exec', timestamp: null, text: 'large output', fullText: '```text\n' + Array.from({ length: 160 }, (_, i) => 'output line ' + i).join('\n') + '\n```', isTruncated: true } }));
   else node = React.createElement(PaneChatView, props);
   window.__chatQaRoot.render(React.createElement(NextIntlClientProvider, { locale: 'ko', messages }, node));
  }, kind);
  await mount('chat');
  const host = page.locator('#chat-feature-qa');
  await host.getByText('현재 대화 응답', { exact: true }).waitFor();
  await expect(host.getByText('자동 주입 지침 QA', { exact: true })).toHaveCount(0);
  await host.getByRole('checkbox', { name: '지침 표시' }).check();
  await host.getByText('자동 주입 지침 QA', { exact: true }).waitFor();
  await host.getByRole('checkbox', { name: '지침 표시' }).uncheck();
  checks.push({ name: 'History 옆 지침 표시 선택', status: 'passed' });
  await host.getByRole('button', { name: '이전 메시지 더 보기' }).click();
  await host.getByText('이전 대화 요청 QA', { exact: true }).waitFor();
  checks.push({ name: '현재 채팅에서 이전 메시지 로딩', status: 'passed' });
  const input = host.getByRole('textbox');
  const before = await input.boundingBox();
  await host.getByRole('button', { name: '입력창 확대' }).click();
  const after = await input.boundingBox();
  expect(after.height).toBeGreaterThan(before.height * 2);
  await input.fill('작업 중 추가 요청 QA');
  await input.press('Enter');
  await expect(host.getByTestId('pane-chat-pending-message')).toContainText('작업 중 추가 요청 QA');
  checks.push({ name: '입력창 확대·작업 중 전송·ACK 전 말풍선', status: 'passed' });
  await app.evaluate(() => globalThis.__chatQa.release());
  await expect(host.getByTestId('pane-chat-pending-message')).toHaveCount(0);
  await expect(host.getByText('작업 중 추가 요청 QA', { exact: true })).toHaveCount(1);
  await host.getByRole('button', { name: modelLabel, exact: true }).click();
  await host.getByRole('combobox', { name: modelLabel, exact: true }).selectOption('qa-model-b');
  await expect(host.getByRole('combobox', { name: modelLabel, exact: true })).toHaveValue('qa-model-b');
  expect(await page.evaluate(() => window.__leftChat ?? false)).toBe(false);
  checks.push({ name: '채팅 유지·모델 API 선택', status: 'passed' });
  await page.screenshot({ path: path.join(run, 'chat-model.png') });
  await mount('settings');
  await host.getByRole('switch', { name: '기본으로 채팅 모드 열기' }).check();
  await expect.poll(() => page.evaluate(() => window.kanvibeDesktop.invoke('appSettings', 'getAppSetting', ['pane_chat_default_enabled']))).toBe('true');
  await mount('layer');
  await host.getByTestId('pane-chat').waitFor();
  await page.reload();
  await page.waitForLoadState('domcontentloaded');
  await mount('layer');
  await host.getByTestId('pane-chat').waitFor();
  checks.push({ name: '설정 저장·기본 채팅·새로고침 후 모드 복원', status: 'passed' });
  await host.getByRole('button', { name: '터미널로 전환', exact: true }).click();
  await expect(host.getByTestId('pane-chat')).toBeHidden();
  await page.reload();
  await page.waitForLoadState('domcontentloaded');
  await mount('layer');
  await expect(host.getByTestId('pane-chat-toggle')).toBeVisible();
  checks.push({ name: '명시적 터미널 모드가 기본 설정보다 우선', status: 'passed' });
  await mount('tool');
  const summary = host.locator('summary');
  await summary.click();
  await host.locator('#tool-scroll').evaluate((el) => { el.scrollTop = 700; });
  await expect.poll(async () => {
   const header = await summary.boundingBox(); const pane = await host.locator('#tool-scroll').boundingBox();
   return Math.abs(header.y - pane.y);
  }).toBeLessThan(4);
  await page.screenshot({ path: path.join(run, 'tool-sticky.png') });
  await summary.click();
  expect(await host.locator('details').evaluate((el) => el.open)).toBe(false);
  checks.push({ name: '도구 exec 머리줄 고정·스크롤 후 접기', status: 'passed' });
 } catch (error) {
  checks.push({ name: 'run', status: 'failed', error: error.stack }); process.exitCode = 1;
  if (page) await page.screenshot({ path: path.join(run, 'failure.png') }).catch(() => {});
 } finally {
  if (app) await app.close();
  fs.writeFileSync(path.join(run, 'results.json'), JSON.stringify(checks, null, 2));
  console.log(JSON.stringify({ run, checks }));
 }
})();
