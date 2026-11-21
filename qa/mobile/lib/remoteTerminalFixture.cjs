/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');

function records(directory, task) {
  try { return fs.readFileSync(path.join(directory, `${task}.jsonl`), 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
}
function nativeActor(mode, task, directory) {
  const transcript = () => records(directory, task).filter((row) => row.type === 'input').map((row) => row.text).join('');
  const screen = () => `\x1b[H\x1b[2JREMOTE ${task.toUpperCase()} READY\r\n${transcript()}`;
  if (mode === '--snapshot') { process.stdout.write(screen()); return; }
  const log = (record) => fs.appendFileSync(path.join(directory, `${task}.jsonl`), `${JSON.stringify(record)}\n`);
  const frame = (text, full, width = 80, height = 24) => process.stdout.write(`${JSON.stringify({ type: 'terminal.frame', bytes: Buffer.from(text).toString('base64'), full, width, height })}\n`);
  log({ type: 'start' }); frame(screen(), true);
  readline.createInterface({ input: process.stdin }).on('line', (line) => {
    const record = JSON.parse(line);
    if (record.type === 'terminal.input') {
      const text = Buffer.from(record.bytes, 'base64').toString('utf8'); log({ type: 'input', text }); frame(text, false);
    } else if (record.type === 'terminal.resize') { log({ type: 'resize', cols: record.cols, rows: record.rows }); frame(screen(), true, record.cols, record.rows); }
    else if (record.type === 'terminal.release') process.exit(0);
  });
  process.stdin.once('end', () => process.exit(0));
}

async function main() {
  const https = require('node:https');
  const http = require('node:http');
  const net = require('node:net');
  const { PassThrough } = require('node:stream');
  const { randomUUID, randomBytes, X509Certificate } = require('node:crypto');
  const { startRemoteTerminalAgent } = require('../../../build/remote/terminal-agent.cjs');
  const { loadOrCreateMobileTlsCertificate } = require('../../../electron/mobileTlsCertificate.js');
  const { handleMobileRequest, attachMobileStreamServer } = require('../../../electron/mobileRoutes.js');
  const directory = path.resolve(process.argv[2] || 'qa-results/roky-harness/mobile-terminal-transport/device-fixture');
  fs.mkdirSync(directory, { recursive: true });
  const apiPort = Number(process.argv[3] || 19880);
  const probePort = Number(process.argv[4] || 19881);
  const [aCertificate, bCertificate] = await Promise.all(['a', 'b'].map((side) => loadOrCreateMobileTlsCertificate(path.join(directory, side))));
  const input = new PassThrough(); const output = new PassThrough();
  const pending = new Map(); const relays = new Map(); const grantTasks = new Map();
  const counts = { direct: { metadata: 0, relayAttaches: 0, relayInputs: 0 }, relay: { metadata: 0, relayAttaches: 0, relayInputs: 0 } };
  let partial = ''; let number = 0; let paired = false;
  output.on('data', (chunk) => {
    partial += chunk.toString(); let end;
    while ((end = partial.indexOf('\n')) >= 0) {
      const record = JSON.parse(partial.slice(0, end)); partial = partial.slice(end + 1);
      if (record.type === 'response') { const waiter = pending.get(record.id); if (waiter) { pending.delete(record.id); clearTimeout(waiter.timer); record.ok ? waiter.resolve() : waiter.reject(new Error(record.error)); } }
      else if (record.type === 'output') relays.get(record.relayId)?.onData(Buffer.from(record.data, 'base64').toString('utf8'));
      else if (record.type === 'closed') { relays.get(record.relayId)?.onClose(record.code, record.reason); relays.delete(record.relayId); }
    }
  });
  const send = (record) => input.write(`${JSON.stringify(record)}\n`);
  const rpc = (method, body) => new Promise((resolve, reject) => {
    const id = String(++number); const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Agent fixture ${method} timed out`)); }, 5000);
    pending.set(id, { resolve, reject, timer }); send({ type: 'request', id, method, ...body });
  });
  const agent = await startRemoteTerminalAgent(bCertificate, input, output);
  const heartbeat = setInterval(() => send({ type: 'heartbeat' }), 5000);
  const blocked = net.createServer((socket) => socket.destroy());
  await new Promise((resolve, reject) => { blocked.once('error', reject); blocked.listen(0, '127.0.0.1', resolve); });
  const fingerprint = new X509Certificate(bCertificate.cert).fingerprint256.replaceAll(':', '').toLowerCase();
  const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
  const tasks = Object.fromEntries(['direct', 'relay'].map((id) => [id, {
    id, title: id === 'direct' ? 'Direct B' : 'Relay B', status: 'todo', projectId: 'transport', projectName: 'Transport QA',
    branchName: `qa/${id}`, sessionType: 'herdr', sessionName: `fixture-${id}`, sshHost: 'fixture-b', worktreePath: '/qa',
  }]));
  const unpaired = new Set(); const noWatch = () => () => {};
  const bridge = {
    authorizeMobileRequest: async (header) => paired && header === 'Bearer fixture-mobile-token' ? 'fixture-device' : null,
    pairMobileDevice: async (code) => { if (code !== '123456') return null; paired = true; return 'fixture-mobile-token'; },
    unpairMobileDeviceByToken: async () => { await revoke(); return true; },
    onMobileDeviceUnpaired: (listener) => { unpaired.add(listener); return () => unpaired.delete(listener); },
    onMobileNotificationsChanged: noWatch, watchTaskSessionChange: noWatch,
    getMobileBoard: async () => ({ projects: [{ id: 'transport', name: 'Transport QA', repoPath: '/qa' }], tasks: { todo: Object.values(tasks), progress: [], pending: [], review: [], done: [] } }),
    getMobileTask: async (id) => tasks[id] || null,
    getMobileNotifications: async () => [], getMobileNotificationSettings: async () => ({ isEnabled: false, enabledStatuses: [] }),
    getTaskSurfaces: async (id) => ({ taskId: id, sessionType: 'herdr', canResize: true, tabs: [{ id: 'w1', name: 'Terminal', panes: [{ id: 'w1:p1', tabId: 'w1', tabName: 'Terminal', command: 'fixture', left: 0, top: 0, width: 80, height: 24 }] }] }),
    isMobileTaskSessionAlive: async () => true,
    prepareMobilePaneConnection: async (task, pane, device) => {
      if (!tasks[task] || pane !== 'w1:p1' || device !== 'fixture-device') throw new Error('Invalid fixture scope');
      counts[task].metadata += 1;
      const grantId = randomUUID(); const token = randomBytes(32).toString('hex'); const expiresAt = Date.now() + 60000;
      const command = `${quote(process.execPath)} ${quote(__filename)}`;
      await rpc('grant', { grant: { grantId, token, expiresAt, deviceId: device, taskId: task, paneId: pane, targetKey: task, cols: 80, rows: 24,
        geometryCommand: `printf '%s\\036%s' ${quote(JSON.stringify({ result: { pane: { pane_id: pane, scroll: { viewport_rows: 24 } } } }))} ${quote(JSON.stringify({ result: { layout: { area: { width: 80 }, zoomed: false, panes: [{ pane_id: pane, rect: { width: 80 } }] } } }))}`,
        controlCommand: `${command} --controller ${quote(task)} ${quote(directory)}`, snapshotCommand: `${command} --snapshot ${quote(task)} ${quote(directory)}` } });
      grantTasks.set(grantId, task);
      return { version: 1, grantId, expiresAt, direct: { host: '10.0.2.2', port: task === 'direct' ? agent.port : blocked.address().port, path: `/stream/${grantId}`, token, certificateFingerprint: fingerprint } };
    },
    openRemoteMobilePaneStream: async (task, pane, device, grantId, onData, onClose) => {
      if (grantTasks.get(grantId) !== task) throw new Error('Invalid fixture grant');
      counts[task].relayAttaches += 1;
      const relayId = randomUUID(); relays.set(relayId, { onData, onClose });
      await rpc('attach', { grantId, taskId: task, paneId: pane, deviceId: device, relayId });
      return { write: (data, binary) => { if (!binary) counts[task].relayInputs += 1; send({ type: 'data', relayId, data: Buffer.from(data).toString('base64'), binary }); }, close: () => { send({ type: 'detach', relayId }); relays.delete(relayId); } };
    },
  };
  async function revoke() { paired = false; await rpc('revokeDevice', { deviceId: 'fixture-device' }); for (const listener of unpaired) listener('fixture-device'); }
  const api = https.createServer(aCertificate, async (request, response) => {
    if (!await handleMobileRequest(request, response, { bridge, host: '127.0.0.1', port: apiPort })) { response.writeHead(404); response.end(); }
  });
  const sockets = attachMobileStreamServer(api, { bridge, host: '127.0.0.1', port: apiPort });
  await new Promise((resolve, reject) => { api.once('error', reject); api.listen(apiPort, '127.0.0.1', resolve); });
  const state = () => ({ runDirectory: directory, apiPort, probePort, pid: process.pid, counts,
    tasks: Object.fromEntries(['direct', 'relay'].map((task) => [task, { controllerStarts: records(directory, task).filter((row) => row.type === 'start').length,
      input: records(directory, task).filter((row) => row.type === 'input').map((row) => row.text).join('') }])) });
  const probe = http.createServer(async (request, response) => {
    const uri = new URL(request.url, `http://127.0.0.1:${probePort}`);
    if (uri.pathname === '/revoke' && request.method === 'POST') await revoke();
    if (uri.pathname === '/state') {
      const marker = uri.searchParams.get('marker'); const task = uri.searchParams.get('task'); const deadline = Date.now() + Math.min(Number(uri.searchParams.get('timeoutMs') || 0), 15000);
      while (marker && task && !state().tasks[task]?.input.includes(marker) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 50));
    }
    const current = state(); fs.writeFileSync(path.join(directory, 'observations.json'), JSON.stringify(current, null, 2));
    response.writeHead(200, { 'Content-Type': 'application/json' }); response.end(JSON.stringify(current));
  });
  await new Promise((resolve, reject) => { probe.once('error', reject); probe.listen(probePort, '127.0.0.1', resolve); });
  fs.writeFileSync(path.join(directory, 'fixture.json'), JSON.stringify({ ...state(), bPort: agent.port, blockedPort: blocked.address().port }, null, 2));
  process.stdout.write(`Remote terminal fixture ready ${apiPort}/${probePort}; run ${directory}\n`);
  let stopped = false;
  function stop() { if (stopped) return; stopped = true; clearInterval(heartbeat); agent.stop(); for (const socket of sockets.clients) socket.terminate(); api.close(); probe.close(); blocked.close(); input.end(); }
  process.on('SIGTERM', stop); process.on('SIGINT', stop);
}
if (process.argv[2] === '--controller' || process.argv[2] === '--snapshot') nativeActor(process.argv[2], process.argv[3], process.argv[4]);
else main().catch((error) => { console.error(error); process.exitCode = 1; });
