// @vitest-environment node
import { execFile } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import http from "node:http";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  buildHookSocketFileName,
  buildRemoteHookSocketPrepareCommand,
  buildShellHookSocketResolver,
  buildShellOwnedHookSocketAssignment,
} from "@/lib/hookSocketPath";
import { buildShellKanvibeStatusUpdater } from "@/lib/shellHookScript";
import { buildTaskCliCommand } from "@/lib/taskCliScript";

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
    await runShell(`mkdir -m 755 '${preparedDirectory}' && : > '${preparedDirectory}/${socketFileName}'`, workspace);

    // When
    const result = await runShell(`${script}; ls -ld '${preparedDirectory}' | cut -c1-10; ls -A '${preparedDirectory}' | wc -l`, workspace);

    // Then
    expect(result.stdout.startsWith(path.join(preparedDirectory, socketFileName))).toBe(true);
    expect(result.stdout).toContain("drwx------");
    expect(result.stdout.trim().endsWith("0")).toBe(true);
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
    function buildCliCommand(targetUrl: string, clientId: string | null = CLIENT_ID): string {
      mkdirSync(path.join(workspace, ".kanvibe"), { recursive: true });
      writeFileSync(
        path.join(workspace, ".kanvibe", "targets.json"),
        JSON.stringify({ targets: [{ url: targetUrl, taskId: "task-1", token: "token-1", clientId }] }),
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

    it("소켓이 없으면 HTTP로 보낸다", async () => {
      // Given
      const viaHttp: ReceivedRequest[] = [];
      const httpServer = startRecordingServer(viaHttp);
      await listen(httpServer, 0, "127.0.0.1");
      const targetUrl = `http://127.0.0.1:${(httpServer.address() as { port: number }).port}`;

      // When
      const result = await runShell(buildCliCommand(targetUrl), workspace);

      // Then
      expect(result.code).toBe(0);
      expect(viaHttp.length).toBe(1);
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

    it("client ID가 없는 옛 항목은 다른 client의 소켓을 쓰지 않고 HTTP로 보낸다", async () => {
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
      expect(result.code).toBe(0);
      expect(viaSocket).toEqual([]);
      expect(viaHttp.length).toBe(1);
    });
  });

  describe("hook 상태 통보", () => {
    function buildHookScript(targetUrl: string): string {
      mkdirSync(path.join(workspace, ".kanvibe"), { recursive: true });
      writeFileSync(
        path.join(workspace, ".kanvibe", "targets.json"),
        JSON.stringify({ targets: [{ url: targetUrl, taskId: "task-1", clientId: CLIENT_ID }] }),
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

    it("소켓이 끊겨 있으면 HTTP로 넘어간다", async () => {
      // Given
      const viaHttp: ReceivedRequest[] = [];
      const httpServer = startRecordingServer(viaHttp);
      await listen(httpServer, 0, "127.0.0.1");
      const port = (httpServer.address() as { port: number }).port;
      const targetUrl = `http://127.0.0.1:${port}`;
      const deadSocket = startRecordingServer([]);
      const socketPath = path.join(socketDirectory, socketFileName);
      await new Promise<void>((resolve) => deadSocket.listen(socketPath, resolve));
      /** ssh가 죽은 뒤처럼 파일은 남기고 듣는 쪽만 없앤다 */
      const keptSocketPath = `${socketPath}.kept`;
      await runShell(`ln '${socketPath}' '${keptSocketPath}'`, workspace);
      await new Promise((resolve) => deadSocket.close(resolve));
      await runShell(`mv '${keptSocketPath}' '${socketPath}'`, workspace);

      // When
      await runShell(buildHookScript(targetUrl), workspace);

      // Then
      expect(viaHttp.map((request) => request.path)).toEqual(["/api/hooks/status"]);
    });
  });
});
