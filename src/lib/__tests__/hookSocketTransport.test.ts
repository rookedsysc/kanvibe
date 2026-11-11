// @vitest-environment node
import { execFile } from "node:child_process";
import { chmodSync, linkSync, mkdirSync, mkdtempSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import http from "node:http";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { ModuleKind, transpileModule } from "typescript";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  buildHookSocketFileName,
  buildRemoteHookSocketPrepareCommand,
  buildShellHookSocketResolver,
  buildShellOwnedHookSocketAssignment,
  buildShellHookTransportResolver,
} from "@/lib/hookSocketPath";
import { buildShellKanvibeStatusUpdater } from "@/lib/shellHookScript";
import { buildTaskCliCommand } from "@/lib/taskCliScript";
import { generatePluginScript } from "@/lib/openCodeHooksSetup";

const CLIENT_ID = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";
/** 원격 스크립트가 targets.json에서 읽는 주소와 무관하게 데스크탑이 만드는 소켓 경로 */
const socketFileName = buildHookSocketFileName(CLIENT_ID);

interface ReceivedRequest {
  path: string;
  body: string;
}

function runShell(script: string, cwd: string): Promise<{ code: number; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    execFile("bash", ["-c", script], { cwd }, (error, stdout, stderr) => {
      resolve({ code: error ? Number(error.code ?? 1) : 0, stdout, stderr });
    });
  });
}

function startRecordingServer(received: ReceivedRequest[]): http.Server {
  return http.createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => {
      body += chunk;
    });
    request.on("end", () => {
      received.push({ path: request.url ?? "", body });
      response.end("{}");
    });
  });
}

describe("hook 터널 소켓 전송", () => {
  let workspace: string;
  let socketDirectory: string;
  const openServers: http.Server[] = [];

  /** 실제 /tmp/kv-<uid>에는 실행 중인 앱의 소켓이 있을 수 있어 테스트 디렉터리로 바꿔 실행한다 */
  const withTestSocketDirectory = (script: string) => script.replaceAll("/tmp/kv-$(id -u)", socketDirectory);

  async function listen(server: http.Server, ...target: [string] | [number, string]): Promise<void> {
    openServers.push(server);
    await new Promise<void>((resolve) => {
      server.listen(...(target as [string]), resolve);
    });
  }

  async function prepareUnavailableSocket(kind: string): Promise<void> {
    const socketPath = path.join(socketDirectory, socketFileName);
    if (kind === "regular") writeFileSync(socketPath, "unexpected file");
    if (kind === "symlink") {
      const targetPath = path.join(socketDirectory, "other.sock");
      await listen(startRecordingServer([]), targetPath);
      symlinkSync(targetPath, socketPath);
    }
    if (kind === "permissions") {
      await listen(startRecordingServer([]), socketPath);
      chmodSync(socketDirectory, 0o755);
    }
    if (kind === "stale") {
      const server = startRecordingServer([]);
      await new Promise<void>((resolve) => server.listen(socketPath, resolve));
      linkSync(socketPath, `${socketPath}.kept`);
      await new Promise<void>((resolve) => server.close(() => resolve()));
      renameSync(`${socketPath}.kept`, socketPath);
    }
  }

  beforeEach(() => {
    /** Unix 소켓 경로는 약 104자로 제한되어, 긴 macOS 임시 디렉터리 대신 실제 소켓처럼 /tmp 아래를 쓴다 */
    workspace = mkdtempSync("/tmp/kvs-");
    socketDirectory = path.join(workspace, "sock");
    mkdirSync(socketDirectory, { mode: 0o700 });
  });

  afterEach(async () => {
    await Promise.all(openServers.splice(0).map((server) => new Promise((resolve) => server.close(resolve))));
    rmSync(workspace, { recursive: true, force: true });
  });

  it("셸이 찾는 소켓 이름과 데스크탑이 만드는 소켓 이름이 같다", async () => {
    // Given
    const socketPath = path.join(socketDirectory, socketFileName);
    await listen(http.createServer(), socketPath);

    // When
    const result = await runShell(
      `${withTestSocketDirectory(buildShellHookSocketResolver())}\nkanvibe_hook_socket '${CLIENT_ID}'`,
      workspace,
    );

    // Then
    expect(result.stdout).toBe(socketPath);
  });

  it.each([
    ["http://localhost:9736", "local", 0],
    ["http://127.0.0.1:9736", "", 0],
    ["http://[::1]:9736", "local", 0],
    ["http://localhost.evil:9736", "local", 1],
    ["http://user@127.0.0.1:9736", "local", 1],
    ["http://localhost:9736@evil", "local", 1],
    ["http://localhost:9736bad", "local", 1],
    ["http://127.0.0.1:9736", "ssh", 1],
    ["http://127.0.0.1:9736", "unknown", 1],
  ])("로컬 HTTP 경계를 검사한다: %s, %s", async (url, transport, expectedCode) => {
    const result = await runShell(`${buildShellHookTransportResolver()}\nkanvibe_hook_uses_local_http '${url}' '${transport}'`, workspace);
    expect(result.code).toBe(expectedCode);
  });

  it.each(["../../etc/x", "a;rm", "", "_"])("경로·셸 문자가 섞였거나 비어 있는 client ID %j로는 소켓을 찾지 않는다", async (clientId) => {
    // Given
    await listen(http.createServer(), path.join(socketDirectory, socketFileName));

    // When
    const result = await runShell(
      `${withTestSocketDirectory(buildShellHookSocketResolver())}\nkanvibe_hook_socket '${clientId}'`,
      workspace,
    );

    // Then
    expect(result.stdout).toBe("");
    expect(() => buildHookSocketFileName(clientId)).toThrow();
  });

  it.each([
    ["다른 사용자도 들어올 수 있는 디렉터리", () => chmodSync(socketDirectory, 0o755)],
    ["소켓이 아닌 일반 파일", () => writeFileSync(path.join(socketDirectory, socketFileName), "")],
  ])("%s의 소켓에는 토큰을 보내지 않는다", async (_name, arrange) => {
    // Given
    if (_name.startsWith("다른")) {
      await listen(http.createServer(), path.join(socketDirectory, socketFileName));
    }
    arrange();

    // When
    const result = await runShell(
      `${withTestSocketDirectory(buildShellHookSocketResolver())}\nkanvibe_hook_socket '${CLIENT_ID}'`,
      workspace,
    );

    // Then
    expect(result.stdout).toBe("");
  });

  it("소켓 디렉터리가 심볼릭 링크면 쓰지 않는다", async () => {
    // Given
    const linkedDirectory = path.join(workspace, "linked");
    symlinkSync(socketDirectory, linkedDirectory);
    await listen(http.createServer(), path.join(socketDirectory, socketFileName));

    // When
    const result = await runShell(
      `${buildShellHookSocketResolver().replaceAll("/tmp/kv-$(id -u)", linkedDirectory)}\nkanvibe_hook_socket '${CLIENT_ID}'`,
      workspace,
    );

    // Then
    expect(result.stdout).toBe("");
  });

  it("준비 명령은 남은 소켓을 지우고 0700 디렉터리를 만든다", async () => {
    // Given
    const preparedDirectory = path.join(workspace, "prepared");
    const script = buildRemoteHookSocketPrepareCommand(CLIENT_ID).replaceAll("/tmp/kv-$(id -u)", preparedDirectory);
    mkdirSync(preparedDirectory, { mode: 0o755 });
    await listen(http.createServer(), path.join(preparedDirectory, socketFileName));

    // When
    const result = await runShell(`${script}; ls -ld '${preparedDirectory}' | cut -c1-10; ls -A '${preparedDirectory}' | wc -l`, workspace);

    // Then
    expect(result.stdout.startsWith(path.join(preparedDirectory, socketFileName))).toBe(true);
    expect(result.stdout).toContain("drwx------");
    expect(result.stdout.trim().endsWith("0")).toBe(true);
  });

  it.each(["regular", "symlink"])("준비 명령은 예상하지 않은 %s 파일을 삭제하지 않는다", async (kind) => {
    const filePath = path.join(socketDirectory, socketFileName);
    const protectedPath = path.join(workspace, "protected");
    writeFileSync(protectedPath, "keep");
    if (kind === "regular") writeFileSync(filePath, "keep");
    else symlinkSync(protectedPath, filePath);

    const result = await runShell(withTestSocketDirectory(buildRemoteHookSocketPrepareCommand(CLIENT_ID)), workspace);

    expect(result.code).not.toBe(0);
    const remaining = await runShell(`cat '${filePath}'`, workspace);
    expect(remaining.stdout).toBe("keep");
  });

  it("한 줄 점검도 0700이 아닌 디렉터리의 소켓은 쓰지 않는다", async () => {
    // Given
    const assignment = withTestSocketDirectory(buildShellOwnedHookSocketAssignment("found", CLIENT_ID));
    await listen(http.createServer(), path.join(socketDirectory, socketFileName));
    chmodSync(socketDirectory, 0o755);

    // When
    const result = await runShell(`${assignment}; printf '%s' "$found"`, workspace);

    // Then
    expect(result.stdout).toBe("");
  });

  it("한 줄 점검은 본인 소유 소켓일 때만 값을 담는다", async () => {
    // Given
    const assignment = withTestSocketDirectory(buildShellOwnedHookSocketAssignment("found", CLIENT_ID));
    const socketPath = path.join(socketDirectory, socketFileName);

    // When
    const missing = await runShell(`${assignment}; printf '%s' "$found"`, workspace);
    await listen(http.createServer(), socketPath);
    const present = await runShell(`${assignment}; printf '%s' "$found"`, workspace);

    // Then
    expect(missing.stdout).toBe("");
    expect(present.stdout).toBe(socketPath);
  });

  describe("task CLI", () => {
    function buildCliCommand(targetUrl: string, clientId: string | null = CLIENT_ID, transport: string | null = "ssh"): string {
      mkdirSync(path.join(workspace, ".kanvibe"), { recursive: true });
      writeFileSync(
        path.join(workspace, ".kanvibe", "targets.json"),
        JSON.stringify({ targets: [{ url: targetUrl, taskId: "task-1", token: "token-1", clientId, transport: transport ?? undefined }] }),
      );
      return withTestSocketDirectory(buildTaskCliCommand(workspace, ["delete", "--yes"]));
    }

    it("소켓이 있으면 HTTP로 보내지 않아 토큰이 평문 포트로 나가지 않는다", async () => {
      // Given
      const viaSocket: ReceivedRequest[] = [];
      const viaHttp: ReceivedRequest[] = [];
      const httpServer = startRecordingServer(viaHttp);
      await listen(httpServer, 0, "127.0.0.1");
      const targetUrl = `http://127.0.0.1:${(httpServer.address() as { port: number }).port}`;
      await listen(startRecordingServer(viaSocket), path.join(socketDirectory, socketFileName));

      // When
      const result = await runShell(buildCliCommand(targetUrl), workspace);

      // Then
      expect(result.code).toBe(0);
      expect(viaSocket.length).toBe(1);
      expect(viaHttp).toEqual([]);
    });

    it.each(["missing", "stale", "regular", "symlink", "permissions"])("원격 소켓이 %s 상태면 HTTP로 보내지 않고 실패한다", async (kind) => {
      // Given
      const viaHttp: ReceivedRequest[] = [];
      const httpServer = startRecordingServer(viaHttp);
      await listen(httpServer, 0, "127.0.0.1");
      const targetUrl = `http://127.0.0.1:${(httpServer.address() as { port: number }).port}`;
      await prepareUnavailableSocket(kind);

      // When
      const result = await runShell(buildCliCommand(targetUrl), workspace);

      // Then
      expect(result.code).not.toBe(0);
      expect(viaHttp).toEqual([]);
    });

    it("PC의 IP가 바뀌어 targets.json의 주소가 낡아도 client ID로 터널 소켓을 찾아 옛 주소로 토큰을 보내지 않는다", async () => {
      // Given
      const viaSocket: ReceivedRequest[] = [];
      const viaStaleAddress: ReceivedRequest[] = [];
      const staleAddressServer = startRecordingServer(viaStaleAddress);
      await listen(staleAddressServer, 0, "127.0.0.1");
      const staleUrl = `http://127.0.0.1:${(staleAddressServer.address() as { port: number }).port}`;
      await listen(startRecordingServer(viaSocket), path.join(socketDirectory, socketFileName));

      // When
      const result = await runShell(buildCliCommand(staleUrl), workspace);

      // Then
      expect(result.code).toBe(0);
      expect(viaSocket.length).toBe(1);
      expect(viaStaleAddress).toEqual([]);
    });

    it("원격 client ID가 없으면 다른 client의 소켓이나 HTTP로 보내지 않는다", async () => {
      // Given
      const viaSocket: ReceivedRequest[] = [];
      const viaHttp: ReceivedRequest[] = [];
      const httpServer = startRecordingServer(viaHttp);
      await listen(httpServer, 0, "127.0.0.1");
      const targetUrl = `http://127.0.0.1:${(httpServer.address() as { port: number }).port}`;
      await listen(startRecordingServer(viaSocket), path.join(socketDirectory, socketFileName));

      // When
      const result = await runShell(buildCliCommand(targetUrl, null), workspace);

      // Then
      expect(result.code).not.toBe(0);
      expect(viaSocket).toEqual([]);
      expect(viaHttp).toEqual([]);
    });

    it.each(["local", null])("로컬 또는 transport 없는 legacy loopback 등록(%s)은 HTTP를 유지한다", async (transport) => {
      const received: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      const url = `http://127.0.0.1:${(server.address() as { port: number }).port}`;
      const command = buildCliCommand(url, null, transport);

      const result = await runShell(command, workspace);

      expect(result.code).toBe(0);
      expect(received).toHaveLength(1);
    });

    it.each(["", "unknown", null, false])("명시적으로 잘못된 transport(%s)는 loopback에서도 TCP로 보내지 않는다", async (transport) => {
      const received: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      const url = `http://127.0.0.1:${(server.address() as { port: number }).port}`;
      const command = buildCliCommand(url, null);
      writeFileSync(path.join(workspace, ".kanvibe", "targets.json"), JSON.stringify({
        targets: [{ url, taskId: "task-1", token: "token-1", transport }],
      }));

      const result = await runShell(command, workspace);

      expect(result.code).not.toBe(0);
      expect(received).toEqual([]);
    });

    it("JSON 필드 순서가 바뀌어도 ssh transport를 잃지 않는다", async () => {
      const received: ReceivedRequest[] = [];
      const viaSocket: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      await listen(startRecordingServer(viaSocket), path.join(socketDirectory, socketFileName));
      const url = `http://127.0.0.1:${(server.address() as { port: number }).port}`;
      const command = buildCliCommand(url, null);
      writeFileSync(path.join(workspace, ".kanvibe", "targets.json"), JSON.stringify({ targets: [
        { transport: "ssh", clientId: CLIENT_ID, taskId: "task-1", token: "token-1", url },
      ] }));

      const result = await runShell(command, workspace);

      expect(result.code).toBe(0);
      expect(viaSocket).toHaveLength(1);
      expect(received).toEqual([]);
    });

    it.each(["local", "unknown", null])("비loopback 또는 userinfo 등록(%s)은 소켓 없이 TCP로 보내지 않는다", async (transport) => {
      const received: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      const url = `http://user@127.0.0.1:${(server.address() as { port: number }).port}`;

      const result = await runShell(buildCliCommand(url, null, transport), workspace);

      expect(result.code).not.toBe(0);
      expect(received).toEqual([]);
    });

    it("같은 worktree의 peer 전파는 각 client의 ssh/local 전송을 유지한다", async () => {
      const remoteTcp: ReceivedRequest[] = [];
      const localTcp: ReceivedRequest[] = [];
      const socket: ReceivedRequest[] = [];
      const remoteServer = startRecordingServer(remoteTcp);
      const localServer = startRecordingServer(localTcp);
      await listen(remoteServer, 0, "127.0.0.1");
      await listen(localServer, 0, "127.0.0.1");
      await listen(startRecordingServer(socket), path.join(socketDirectory, socketFileName));
      mkdirSync(path.join(workspace, ".kanvibe"), { recursive: true });
      writeFileSync(path.join(workspace, ".kanvibe", "targets.json"), JSON.stringify({ targets: [
        { url: `http://127.0.0.1:${(remoteServer.address() as { port: number }).port}`, taskId: "remote-task", token: "remote-token", clientId: CLIENT_ID, transport: "ssh" },
        { url: `http://127.0.0.1:${(localServer.address() as { port: number }).port}`, taskId: "local-task", token: "local-token", transport: "local" },
      ] }));

      const result = await runShell(withTestSocketDirectory(buildTaskCliCommand(workspace, ["propagate-delete", "--exclude-url", "http://localhost:1"])), workspace);

      expect(result.code).toBe(0);
      expect(remoteTcp).toEqual([]);
      expect(socket.map((request) => JSON.parse(request.body).contextTaskId)).toEqual(["remote-task"]);
      expect(localTcp.map((request) => JSON.parse(request.body).contextTaskId)).toEqual(["local-task"]);
    });
  });

  describe("hook 상태 통보", () => {
    function buildHookScript(targetUrl: string, transport: string = "ssh"): string {
      mkdirSync(path.join(workspace, ".kanvibe"), { recursive: true });
      writeFileSync(
        path.join(workspace, ".kanvibe", "targets.json"),
        JSON.stringify({ targets: [{ url: targetUrl, taskId: "task-1", clientId: CLIENT_ID, transport }] }),
      );
      return `TASK_ID=task-1\nKANVIBE_URL=${targetUrl}\nset --\ncd '${workspace}'\ngit init -q 2>/dev/null\n${withTestSocketDirectory(buildShellKanvibeStatusUpdater("review"))}`;
    }

    it("소켓이 있으면 HTTP 대신 소켓으로 보낸다", async () => {
      // Given
      const viaSocket: ReceivedRequest[] = [];
      const viaHttp: ReceivedRequest[] = [];
      const httpServer = startRecordingServer(viaHttp);
      await listen(httpServer, 0, "127.0.0.1");
      const port = (httpServer.address() as { port: number }).port;
      const targetUrl = `http://127.0.0.1:${port}`;
      await listen(startRecordingServer(viaSocket), path.join(socketDirectory, socketFileName));

      // When
      await runShell(buildHookScript(targetUrl), workspace);

      // Then
      expect(viaSocket.map((request) => request.path)).toEqual(["/api/hooks/status"]);
      expect(JSON.parse(viaSocket[0].body)).toEqual({ taskId: "task-1", status: "review" });
      expect(viaHttp).toEqual([]);
    });

    it.each(["missing", "stale", "regular", "symlink", "permissions"])("원격 소켓이 %s 상태여도 HTTP로 보내지 않는다", async (kind) => {
      // Given
      const viaHttp: ReceivedRequest[] = [];
      const httpServer = startRecordingServer(viaHttp);
      await listen(httpServer, 0, "127.0.0.1");
      const port = (httpServer.address() as { port: number }).port;
      const targetUrl = `http://127.0.0.1:${port}`;
      await prepareUnavailableSocket(kind);

      // When
      await runShell(buildHookScript(targetUrl), workspace);

      // Then
      expect(viaHttp).toEqual([]);
    });

    it.each(["", "unknown"])("잘못된 transport(%s)는 loopback 상태도 TCP로 보내지 않는다", async (transport) => {
      const received: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      const url = `http://127.0.0.1:${(server.address() as { port: number }).port}`;

      await runShell(buildHookScript(url, transport), workspace);

      expect(received).toEqual([]);
    });

    it("JSON 필드 순서가 바뀌어도 ssh 상태를 TCP로 보내지 않는다", async () => {
      const received: ReceivedRequest[] = [];
      const viaSocket: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      await listen(startRecordingServer(viaSocket), path.join(socketDirectory, socketFileName));
      const url = `http://127.0.0.1:${(server.address() as { port: number }).port}`;
      const script = buildHookScript(url);
      writeFileSync(path.join(workspace, ".kanvibe", "targets.json"), JSON.stringify({ targets: [
        { transport: "ssh", clientId: CLIENT_ID, taskId: "task-1", url },
      ] }));

      await runShell(script, workspace);

      expect(viaSocket).toHaveLength(1);
      expect(received).toEqual([]);
    });

    it("로컬 loopback 등록은 HTTP로 상태를 보낸다", async () => {
      const received: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      const url = `http://127.0.0.1:${(server.address() as { port: number }).port}`;

      await runShell(buildHookScript(url, "local"), workspace);

      expect(received.map((request) => request.path)).toEqual(["/api/hooks/status"]);
    });
  });

  describe("OpenCode 생성 플러그인", () => {
    async function notify(targetUrl: string, transport?: string): Promise<void> {
      mkdirSync(path.join(workspace, ".kanvibe"), { recursive: true });
      writeFileSync(path.join(workspace, ".kanvibe", "targets.json"), JSON.stringify({
        targets: [{ url: targetUrl, taskId: "task-1", clientId: CLIENT_ID, transport }],
      }));
      const pluginPath = path.join(workspace, ".opencode", "plugins", "kanvibe-plugin.ts");
      const source = generatePluginScript(targetUrl, "task-1")
        .replaceAll("import.meta.url", JSON.stringify(pathToFileURL(pluginPath).href))
        .replace('"/tmp/kv-" + uid', JSON.stringify(socketDirectory));
      const compiled = transpileModule(source, { compilerOptions: { module: ModuleKind.CommonJS } }).outputText;
      const exports: { KanvibePlugin?: (context: unknown) => Promise<{ event: (input: unknown) => Promise<void> }> } = {};
      /** Node의 fetch에는 Bun unix 옵션이 없어, HTTP 어댑터 경계만 실제 socketPath 요청으로 연결한다. */
      const fetchOverHttp = (url: string, init: { method: string; body: string; headers: Record<string, string>; unix?: string }) => new Promise((resolve, reject) => {
        const request = http.request(url, { method: init.method, headers: init.headers, socketPath: init.unix }, (response) => {
          response.resume();
          response.once("end", () => resolve({ ok: response.statusCode === 200, status: response.statusCode }));
        });
        request.once("error", reject);
        request.end(init.body);
      });
      new Function("require", "exports", "fetch", compiled)(createRequire(import.meta.url), exports, fetchOverHttp);
      const plugin = await exports.KanvibePlugin!({});
      await plugin.event({ event: { type: "session.idle", properties: { sessionID: "session-1", parentID: "" } } });
    }

    it("ssh 등록은 실제 Unix 소켓으로만 상태를 보낸다", async () => {
      const viaHttp: ReceivedRequest[] = [];
      const viaSocket: ReceivedRequest[] = [];
      const server = startRecordingServer(viaHttp);
      await listen(server, 0, "127.0.0.1");
      await listen(startRecordingServer(viaSocket), path.join(socketDirectory, socketFileName));

      await notify(`http://127.0.0.1:${(server.address() as { port: number }).port}`, "ssh");

      expect(viaSocket.map((request) => request.path)).toEqual(["/api/hooks/status"]);
      expect(viaHttp).toEqual([]);
    });

    it.each(["missing", "stale", "regular", "symlink", "permissions"])("ssh 소켓이 %s 상태면 응답하는 TCP 서버에도 보내지 않는다", async (kind) => {
      const received: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");
      await prepareUnavailableSocket(kind);

      await notify(`http://127.0.0.1:${(server.address() as { port: number }).port}`, "ssh");

      expect(received).toEqual([]);
    });

    it.each(["local", undefined])("로컬/transport 없는 legacy loopback 등록(%s)은 HTTP로 보낸다", async (transport) => {
      const received: ReceivedRequest[] = [];
      const server = startRecordingServer(received);
      await listen(server, 0, "127.0.0.1");

      await notify(`http://127.0.0.1:${(server.address() as { port: number }).port}`, transport);

      expect(received.map((request) => request.path)).toEqual(["/api/hooks/status"]);
    });
  });
});
