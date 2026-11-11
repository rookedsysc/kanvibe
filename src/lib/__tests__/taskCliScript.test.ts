/**
 * @vitest-environment node
 */
import { execFile } from "node:child_process";
import { access, chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildTaskCliCommand, buildTaskCliFile } from "@/lib/taskCliScript";

const execFileAsync = promisify(execFile);
const require = createRequire(import.meta.url);
const { handleTaskCliRequest } = require("../../../electron/taskCliRoutes.js") as {
  handleTaskCliRequest: (
    request: http.IncomingMessage,
    response: http.ServerResponse,
    dependencies: { taskCli: Record<string, (authorization: string | undefined, body: unknown) => Promise<unknown>> },
  ) => Promise<boolean>;
};

interface RecordedCall {
  functionName: string;
  authorization: string | undefined;
  body: Record<string, unknown>;
}

interface FakeClient {
  url: string;
  calls: RecordedCall[];
  close: () => Promise<void>;
}

interface CliRun {
  exitCode: number;
  stdout: string;
  stderr: string;
}

let workspaceDir: string;
let repoDir: string;
let openClients: FakeClient[];

/** 실제 hook 서버와 같은 task CLI 라우터를 쓰고, 서비스만 호출 기록으로 바꾼 client */
async function startFakeClient(respond?: (call: RecordedCall) => unknown): Promise<FakeClient> {
  const calls: RecordedCall[] = [];
  const recordCall = (functionName: string) => async (authorization: string | undefined, body: unknown) => {
    const call = { functionName, authorization, body: body as Record<string, unknown> };
    calls.push(call);
    return respond ? respond(call) : { handledBy: functionName };
  };
  const server = http.createServer(async (request, response) => {
    await handleTaskCliRequest(request, response, {
      taskCli: {
        createTaskFromCli: recordCall("createTaskFromCli"),
        deleteTaskFromCli: recordCall("deleteTaskFromCli"),
        registerTaskFromPeer: recordCall("registerTaskFromPeer"),
      },
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address() as { port: number };
  const client = {
    url: `http://127.0.0.1:${port}`,
    calls,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  };
  openClients.push(client);
  return client;
}

/** 열었다 바로 닫아 연결이 거부되는 주소를 얻는다 */
async function reserveClosedUrl(): Promise<string> {
  const client = await startFakeClient();
  await client.close();
  openClients = openClients.filter((candidate) => candidate !== client);
  return client.url;
}

async function writeTargets(targets: Array<{ url: string; taskId: string; token?: string }>): Promise<void> {
  await writeFile(path.join(repoDir, ".kanvibe", "targets.json"), JSON.stringify({ schemaVersion: 1, targets }, null, 2));
}

/**
 * 꺼진 PC처럼 패킷을 버리는 주소는 로컬에서 재현할 수 없어, 그 주소로 가는 요청만 curl의 연결 시간 초과처럼 끝내는 curl을 PATH 앞에 둔다.
 * 실제 curl도 연결 전 시간 초과면 time_connect 0, 요청이 나간 뒤의 시간 초과면 0보다 큰 time_connect와 함께 28로 끝난다.
 */
async function createTimingOutCurlPath(timingOutUrl: string, connectSeconds: string): Promise<string> {
  const { stdout: realCurlPath } = await execFileAsync("bash", ["-c", "command -v curl"]);
  const fakeBinDir = path.join(workspaceDir, "fake bin");
  await mkdir(fakeBinDir, { recursive: true });
  const fakeCurlPath = path.join(fakeBinDir, "curl");
  await writeFile(fakeCurlPath, [
    "#!/usr/bin/env bash",
    "for argument in \"$@\"; do",
    `  if [[ "$argument" == ${JSON.stringify(timingOutUrl)}* ]]; then`,
    "    cat >/dev/null",
    `    printf '\\n%s\\n000' ${JSON.stringify(connectSeconds)}`,
    "    exit 28",
    "  fi",
    "done",
    `exec ${JSON.stringify(realCurlPath.trim())} "$@"`,
    "",
  ].join("\n"));
  await chmod(fakeCurlPath, 0o755);
  return `${fakeBinDir}${path.delimiter}${process.env.PATH ?? ""}`;
}

async function runCli(args: string[], cwd = repoDir, pathOverride?: string): Promise<CliRun> {
  const scriptPath = path.relative(cwd, path.join(repoDir, ".kanvibe", "task.sh"));
  const env = pathOverride ? { ...process.env, PATH: pathOverride } : undefined;
  try {
    const { stdout, stderr } = await execFileAsync("bash", [scriptPath, ...args], { cwd, env });
    return { exitCode: 0, stdout, stderr };
  } catch (error) {
    const failure = error as { code: number; stdout: string; stderr: string };
    return { exitCode: failure.code, stdout: failure.stdout, stderr: failure.stderr };
  }
}

async function git(args: string[]): Promise<void> {
  await execFileAsync("git", ["-C", repoDir, ...args]);
}

describe("task CLI script", () => {
  beforeEach(async () => {
    openClients = [];
    workspaceDir = await mkdtemp(path.join(tmpdir(), "kanvibe cli "));
    repoDir = path.join(workspaceDir, "repo with space");
    await mkdir(path.join(repoDir, "nested dir"), { recursive: true });
    await git(["init", "-q", "-b", "feat/current"]);
    await git(["-c", "user.name=kanvibe", "-c", "user.email=kanvibe@example.com", "commit", "-q", "--allow-empty", "-m", "init"]);

    const taskCliFile = buildTaskCliFile(repoDir);
    await mkdir(path.dirname(taskCliFile.filePath), { recursive: true });
    await writeFile(taskCliFile.filePath, taskCliFile.content);
    await chmod(taskCliFile.filePath, taskCliFile.mode!);
  });

  afterEach(async () => {
    await Promise.all(openClients.map((client) => client.close()));
    await rm(workspaceDir, { recursive: true, force: true });
    vi.restoreAllMocks();
  });

  describe("create", () => {
    it("하위 디렉터리에서 실행해도 task worktree의 현재 브랜치를 기준으로 첫 client 한 곳에만 요청한다", async () => {
      const firstClient = await startFakeClient();
      const secondClient = await startFakeClient();
      await writeTargets([
        { url: `${firstClient.url}/`, taskId: "task-first", token: "token-first" },
        { url: secondClient.url, taskId: "task-second", token: "token-second" },
      ]);

      const run = await runCli(
        ["create", "--branch", "feat/new", "--title", "로그인 \"수정\" $HOME `id` \\ 끝", "--session-type", "tmux"],
        path.join(repoDir, "nested dir"),
      );

      expect(run.exitCode).toBe(0);
      expect(JSON.parse(run.stdout)).toEqual({ success: true, data: { handledBy: "createTaskFromCli" } });
      expect(firstClient.calls).toEqual([{
        functionName: "createTaskFromCli",
        authorization: "Bearer token-first",
        body: {
          contextTaskId: "task-first",
          branchName: "feat/new",
          title: "로그인 \"수정\" $HOME `id` \\ 끝",
          sessionType: "tmux",
          baseBranch: "feat/current",
        },
      }]);
      expect(secondClient.calls).toEqual([]);
    });

    it("제목의 제어 문자도 JSON 이스케이프해 client가 본문을 그대로 읽는다", async () => {
      const client = await startFakeClient();
      await writeTargets([{ url: client.url, taskId: "task-1", token: "token-1" }]);
      const title = "줄\n바꿈\t탭\u0001제어\u001f끝 \"따옴표\" \\";

      const run = await runCli(["create", "--branch", "feat/new", "--title", title]);

      expect(run.exitCode).toBe(0);
      expect(client.calls[0].body.title).toBe(title);
    });

    it("--from-task를 주면 현재 브랜치 대신 그 task id를 보내 client가 기준 브랜치를 정하게 한다", async () => {
      const client = await startFakeClient();
      await writeTargets([{ url: client.url, taskId: "task-1", token: "token-1" }]);
      await git(["checkout", "-q", "--detach"]);

      const run = await runCli(["create", "--branch", "feat/followup", "--from-task", "base-task"]);

      expect(run.exitCode).toBe(0);
      expect(client.calls[0].body).toEqual({ contextTaskId: "task-1", branchName: "feat/followup", fromTaskId: "base-task" });
    });

    it("detached HEAD에서 기준 task 없이 만들면 요청을 보내지 않고 사용 오류로 끝난다", async () => {
      const client = await startFakeClient();
      await writeTargets([{ url: client.url, taskId: "task-1", token: "token-1" }]);
      await git(["checkout", "-q", "--detach"]);

      const run = await runCli(["create", "--branch", "feat/new"]);

      expect(run.exitCode).toBe(2);
      expect(run.stderr).toContain("detached HEAD");
      expect(client.calls).toEqual([]);
    });

    it.each([
      [["create"], "--branch는 필수입니다"],
      [["create", "--branch"], "--branch 에 값이 필요합니다"],
      [["create", "--branch", "feat/new", "--unknown"], "알 수 없는 옵션입니다: --unknown"],
    ])("잘못된 인자 %j 는 요청 없이 사용 오류로 끝난다", async (args, message) => {
      const client = await startFakeClient();
      await writeTargets([{ url: client.url, taskId: "task-1", token: "token-1" }]);

      const run = await runCli(args);

      expect(run.exitCode).toBe(2);
      expect(run.stderr).toContain(message);
      expect(client.calls).toEqual([]);
    });

    it("첫 client에 연결할 수 없을 때만 다음 client로 넘어간다", async () => {
      const closedUrl = await reserveClosedUrl();
      const reachableClient = await startFakeClient();
      await writeTargets([
        { url: closedUrl, taskId: "task-offline", token: "token-offline" },
        { url: reachableClient.url, taskId: "task-online", token: "token-online" },
      ]);

      const run = await runCli(["create", "--branch", "feat/new"]);

      expect(run.exitCode).toBe(0);
      expect(run.stderr).toContain("연결할 수 없어 다음 client를 시도합니다");
      expect(reachableClient.calls.map((call) => call.body.contextTaskId)).toEqual(["task-online"]);
    });

    it("응답 없는 client는 연결 시간 초과로 보고 다음 client로 넘어간다", async () => {
      const silentUrl = "http://localhost:9736";
      const reachableClient = await startFakeClient();
      await writeTargets([
        { url: silentUrl, taskId: "task-sleeping", token: "token-sleeping" },
        { url: reachableClient.url, taskId: "task-online", token: "token-online" },
      ]);

      const run = await runCli(["create", "--branch", "feat/new"], repoDir, await createTimingOutCurlPath(silentUrl, "0.000000"));

      expect(run.exitCode).toBe(0);
      expect(run.stderr).toContain(`${silentUrl} 에 연결할 수 없어 다음 client를 시도합니다`);
      expect(reachableClient.calls.map((call) => call.body.contextTaskId)).toEqual(["task-online"]);
    });

    it("연결된 뒤 응답을 기다리다 시간이 다 되면 이미 처리됐을 수 있으므로 다른 client로 다시 보내지 않는다", async () => {
      const slowUrl = "http://localhost:9736";
      const otherClient = await startFakeClient();
      await writeTargets([
        { url: slowUrl, taskId: "task-slow", token: "token-slow" },
        { url: otherClient.url, taskId: "task-other", token: "token-other" },
      ]);

      const run = await runCli(["create", "--branch", "feat/new"], repoDir, await createTimingOutCurlPath(slowUrl, "0.000349"));

      expect(run.exitCode).toBe(1);
      expect(run.stderr).toContain("curl 28");
      expect(otherClient.calls).toEqual([]);
    });

    it("응답을 받은 실패는 다른 client로 다시 보내지 않고 오류 본문과 함께 실패로 끝난다", async () => {
      const failingClient = await startFakeClient(() => {
        throw Object.assign(new Error("이미 같은 브랜치의 task가 있습니다: feat/new"), { name: "TaskCliRequestError", statusCode: 409 });
      });
      const otherClient = await startFakeClient();
      await writeTargets([
        { url: failingClient.url, taskId: "task-1", token: "token-1" },
        { url: otherClient.url, taskId: "task-2", token: "token-2" },
      ]);

      const run = await runCli(["create", "--branch", "feat/new"]);

      expect(run.exitCode).toBe(1);
      expect(run.stdout).toBe("");
      expect(run.stderr).toContain("HTTP 409");
      expect(run.stderr).toContain("이미 같은 브랜치의 task가 있습니다");
      expect(otherClient.calls).toEqual([]);
    });

    it("--target-url로 고른 client에만 보내고, 등록되지 않은 주소면 사용 오류로 끝난다", async () => {
      const firstClient = await startFakeClient();
      const secondClient = await startFakeClient();
      await writeTargets([
        { url: firstClient.url, taskId: "task-1", token: "token-1" },
        { url: secondClient.url, taskId: "task-2", token: "token-2" },
      ]);

      const selectedRun = await runCli(["create", "--branch", "feat/new", "--target-url", `${secondClient.url}/`]);
      const unknownRun = await runCli(["create", "--branch", "feat/new", "--target-url", "http://127.0.0.1:1"]);

      expect(selectedRun.exitCode).toBe(0);
      expect(firstClient.calls).toEqual([]);
      expect(secondClient.calls.map((call) => call.authorization)).toEqual(["Bearer token-2"]);
      expect(unknownRun.exitCode).toBe(2);
      expect(unknownRun.stderr).toContain("등록된 client가 아닙니다");
    });

    it("CLI 토큰이 없는 예전 등록만 있으면 요청하지 않고 hooks 재설치를 안내한다", async () => {
      const client = await startFakeClient();
      await writeTargets([{ url: client.url, taskId: "task-1" }]);

      const run = await runCli(["create", "--branch", "feat/new"]);

      expect(run.exitCode).toBe(1);
      expect(run.stderr).toContain("hooks를 다시 설치하세요");
      expect(client.calls).toEqual([]);
    });
  });

  describe("delete", () => {
    it("--yes 없이는 아무 client에도 요청하지 않는다", async () => {
      const client = await startFakeClient();
      await writeTargets([{ url: client.url, taskId: "task-1", token: "token-1" }]);

      const run = await runCli(["delete"]);

      expect(run.exitCode).toBe(2);
      expect(run.stderr).toContain("--yes");
      expect(client.calls).toEqual([]);
    });

    it("확인과 강제 여부, 지울 브랜치를 한 client에 보낸다", async () => {
      const firstClient = await startFakeClient();
      const secondClient = await startFakeClient();
      await writeTargets([
        { url: firstClient.url, taskId: "task-1", token: "token-1" },
        { url: secondClient.url, taskId: "task-2", token: "token-2" },
      ]);

      const defaultRun = await runCli(["delete", "--yes"]);
      const forcedRun = await runCli(["delete", "--yes", "--force", "--branch", "feat/sibling"]);

      expect([defaultRun.exitCode, forcedRun.exitCode]).toEqual([0, 0]);
      expect(firstClient.calls.map((call) => call.body)).toEqual([
        { contextTaskId: "task-1", confirmed: true, force: false },
        { contextTaskId: "task-1", confirmed: true, force: true, branchName: "feat/sibling" },
      ]);
      expect(secondClient.calls).toEqual([]);
    });

    it("다른 client로 전파하지 못한 곳이 있으면 삭제는 성공으로 끝내되 남은 client 수를 경고한다", async () => {
      const client = await startFakeClient(() => ({
        id: "task-1",
        deleted: true,
        peers: [
          { url: "http://10.0.0.7:9736", isSuccess: true, httpStatus: "200", message: "ok" },
          { url: "http://10.0.0.9:9736", isSuccess: false, httpStatus: "000", message: "연결할 수 없습니다" },
        ],
      }));
      await writeTargets([{ url: client.url, taskId: "task-1", token: "token-1" }]);

      const run = await runCli(["delete", "--yes"]);

      expect(run.exitCode).toBe(0);
      expect(JSON.parse(run.stdout).data.deleted).toBe(true);
      expect(run.stderr).toContain("1곳에는 삭제를 전파하지 못했습니다");
    });

    it("모든 client에 전파되면 경고하지 않는다", async () => {
      const client = await startFakeClient(() => ({
        id: "task-1",
        deleted: true,
        peers: [{ url: "http://10.0.0.7:9736", isSuccess: true, httpStatus: "200", message: "ok" }],
      }));
      await writeTargets([{ url: client.url, taskId: "task-1", token: "token-1" }]);

      const run = await runCli(["delete", "--yes"]);

      expect(run.exitCode).toBe(0);
      expect(run.stderr).toBe("");
    });
  });

  describe("client 사이 전파", () => {
    it("propagate-delete는 자기 자신을 뺀 모든 client에 각자의 taskId와 토큰으로 재전파 없는 강제 삭제를 보낸다", async () => {
      const ownClient = await startFakeClient();
      const peerClient = await startFakeClient();
      const closedUrl = await reserveClosedUrl();
      await writeTargets([
        { url: ownClient.url, taskId: "task-own", token: "token-own" },
        { url: peerClient.url, taskId: "task-peer", token: "token-peer" },
        { url: closedUrl, taskId: "task-offline", token: "token-offline" },
      ]);

      const run = await runCli(["propagate-delete", "--exclude-url", ownClient.url]);

      expect(run.exitCode).toBe(0);
      expect(ownClient.calls).toEqual([]);
      expect(peerClient.calls).toEqual([{
        functionName: "deleteTaskFromCli",
        authorization: "Bearer token-peer",
        body: { contextTaskId: "task-peer", confirmed: true, force: true, propagateToPeers: false },
      }]);
      expect(run.stdout.trim().split("\n")).toEqual([
        `__KANVIBE_PEER_RESULT__\t${peerClient.url}\t200\t{"success":true,"data":{"handledBy":"deleteTaskFromCli"}}`,
        `__KANVIBE_PEER_RESULT__\t${closedUrl}\t000\t연결할 수 없습니다`,
      ]);
    });

    it("propagate-register는 다른 client에 새 브랜치 등록만 요청한다", async () => {
      const ownClient = await startFakeClient();
      const peerClient = await startFakeClient();
      await writeTargets([
        { url: ownClient.url, taskId: "root-own", token: "token-own" },
        { url: peerClient.url, taskId: "root-peer", token: "token-peer" },
      ]);

      const run = await runCli(["propagate-register", "--exclude-url", ownClient.url, "--branch", "feat/new"]);

      expect(run.exitCode).toBe(0);
      expect(peerClient.calls).toEqual([{
        functionName: "registerTaskFromPeer",
        authorization: "Bearer token-peer",
        body: { contextTaskId: "root-peer", branchName: "feat/new" },
      }]);
    });
  });

  it("토큰은 명령줄 인자로 curl에 넘기지 않는다", () => {
    const { content } = buildTaskCliFile("/repo");

    expect(content).toContain("printf 'Authorization: Bearer %s\\n' \"${token}\" | curl");
    expect(content).toContain("-H @-");
    expect(content).not.toMatch(/-H\s+["']Authorization/);
  });

  describe("KanVibe client가 실행하는 경로", () => {
    const REPLACED_SCRIPT_MARKER = "replaced-script-ran";

    /** 앱이 설치한 task.sh가 ignored 상태일 때 그 파일을 추적하는 브랜치를 checkout해 git이 덮어쓰게 한다 */
    async function checkoutBranchThatReplacesTaskCli(): Promise<void> {
      const taskCliPath = path.join(repoDir, ".kanvibe", "task.sh");
      const trustedScript = await readFile(taskCliPath, "utf-8");
      const commitArguments = ["-c", "user.name=kanvibe", "-c", "user.email=kanvibe@example.com", "commit", "-q", "-m", "tracked task.sh"];
      await writeFile(path.join(repoDir, ".git", "info", "exclude"), ".kanvibe/\n");
      await git(["checkout", "-q", "-b", "untrusted"]);
      await writeFile(taskCliPath, `#!/bin/bash\ntouch ${JSON.stringify(path.join(workspaceDir, REPLACED_SCRIPT_MARKER))}\n`);
      await git(["add", "-f", ".kanvibe/task.sh"]);
      await git(commitArguments);
      await git(["checkout", "-q", "feat/current"]);
      await writeFile(taskCliPath, trustedScript);
      await git(["checkout", "-q", "untrusted"]);
      expect(await readFile(taskCliPath, "utf-8")).toContain(REPLACED_SCRIPT_MARKER);
    }

    it("덮어써진 task.sh 대신 앱이 가진 본문을 실행한다", async () => {
      // Given
      const peer = await startFakeClient();
      await writeTargets([{ url: peer.url, taskId: "task-peer", token: "peer-token" }]);
      await checkoutBranchThatReplacesTaskCli();
      const command = buildTaskCliCommand(repoDir, ["propagate-delete", "--exclude-url", "http://own:9736"]);

      // When
      const { stdout } = await execFileAsync("sh", ["-c", command]);

      // Then
      await expect(access(path.join(workspaceDir, REPLACED_SCRIPT_MARKER))).rejects.toThrow();
      expect(peer.calls.map((call) => call.functionName)).toEqual(["deleteTaskFromCli"]);
      expect(stdout).toContain(`__KANVIBE_PEER_RESULT__\t${peer.url}\t200`);
    });

    it("원격 실행처럼 셸을 두 번 거쳐도 본문과 인자가 그대로 전달된다", async () => {
      // Given
      const peer = await startFakeClient();
      await writeTargets([{ url: peer.url, taskId: "task-peer", token: "peer-token" }]);
      const command = buildTaskCliCommand(repoDir, ["propagate-register", "--branch", "feat/it's new"]);
      const quote = (value: string) => `'${value.replace(/'/g, `'"'"'`)}'`;

      // When
      await execFileAsync("sh", ["-lc", `sh -lc ${quote(`sh -lc ${quote(command)}`)}`]);

      // Then
      expect(peer.calls[0]).toMatchObject({
        functionName: "registerTaskFromPeer",
        body: { contextTaskId: "task-peer", branchName: "feat/it's new" },
      });
    });

    it("targets.json의 url이 curl 옵션이나 http가 아닌 주소면 호출하지 않는다", async () => {
      // Given
      const curlConfigDir = path.join(workspaceDir, "cfg");
      const writtenPath = path.join(workspaceDir, "WRITTEN");
      await mkdir(path.join(curlConfigDir, "api", "cli", "tasks"), { recursive: true });
      await writeFile(
        path.join(curlConfigDir, "api", "cli", "tasks", "delete"),
        `url = "file:///etc/hosts"\noutput = ${JSON.stringify(writtenPath)}\n`,
      );
      await writeTargets([
        { url: `-K${curlConfigDir}`, taskId: "task-option", token: "token" },
        { url: "file:///etc/hosts", taskId: "task-file", token: "token" },
      ]);
      const command = buildTaskCliCommand(repoDir, ["propagate-delete", "--exclude-url", "http://own:9736"]);

      // When
      const { stdout } = await execFileAsync("sh", ["-c", command]);

      // Then
      await expect(access(writtenPath)).rejects.toThrow();
      expect(stdout).not.toContain("__KANVIBE_PEER_RESULT__");
    });
  });
});
