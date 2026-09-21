/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const http = require("node:http");
const net = require("node:net");
const path = require("node:path");
const { execFileSync, spawn } = require("node:child_process");

/**
 * 모바일 Maestro 여정이 상대할 데스크탑 한 대를 준비하고, 여정이 끝날 때까지 살아 있는다.
 *
 * 사용자의 실제 보드가 아니라 run 디렉터리 안의 격리된 app data를 쓴다. 실제 DB를 쓰면 여정이
 * 상태를 옮길 때마다 사용자의 칸반이 흔들리고, 남는 worktree와 tmux 세션도 사용자 프로젝트에 쌓인다.
 *
 * 준비가 끝나면 pane 내용을 확인해 주는 작은 probe 서버를 연다. xterm은 터미널 내용을 접근성
 * 트리에 내놓지 않아서, 모바일에서 친 키가 실제로 pane에 닿았는지는 기기 화면만 봐서는 단정할 수 없다.
 */

const DESKTOP_API = "http://127.0.0.1:19736";
const PROBE_PORT = 19737;
const FIXTURE_PROJECT_NAME = "kanvibe-qa-fixture";
const FIXTURE_PROJECT_ID = "00000000-0000-4000-8000-000000000001";
const FIXTURE_BRANCH = "maestro/fixture";

/** 플로우가 셀렉터로 그대로 쓰는 값이라 사람이 읽고 옮겨 적을 수 있게 상수로 둔다 */
const NO_SESSION_TASK_TITLE = "Maestro Fixture NoSession";
const SESSION_TASK_TITLE = "Maestro Fixture Session";

/** 데스크탑이 비추는 pane에 미리 찍어 두는 표식. 기기 화면에 이 글자가 보이면 미러링이 살아 있는 것이다 */
const PANE_READY_MARKER = "MAESTRO_PANE_READY";

const DESKTOP_READY_TIMEOUT_MS = 240_000;
const DESKTOP_POLL_INTERVAL_MS = 1_000;
const MARKER_POLL_INTERVAL_MS = 500;
const DESKTOP_SIGKILL_DELAY_MS = 2_000;

/** `scripts/run-desktop-dev.cjs`가 먼저 노려 보는 vite 포트 */
const PREFERRED_DEV_SERVER_PORT = 5173;

function run(command, args, options = {}) {
  return execFileSync(command, args, { encoding: "utf8", ...options }).trim();
}

function createFixtureRepository(runDir) {
  const repoPath = path.join(runDir, FIXTURE_PROJECT_NAME);
  fs.mkdirSync(repoPath, { recursive: true });

  run("git", ["init", "-b", "main"], { cwd: repoPath });
  run("git", ["config", "user.email", "qa@kanvibe.local"], { cwd: repoPath });
  run("git", ["config", "user.name", "KanVibe QA"], { cwd: repoPath });
  fs.writeFileSync(path.join(repoPath, "README.md"), "KanVibe mobile QA fixture\n");
  run("git", ["add", "."], { cwd: repoPath });
  run("git", ["commit", "-m", "chore: qa fixture"], { cwd: repoPath });

  return repoPath;
}

/**
 * 데스크탑이 vite 기본 포트를 피해 가게 한다.
 *
 * 5173은 다른 프로젝트의 개발 서버가 흔히 쓰는 자리다. 남이 와일드카드로 잡고 있으면 데스크탑 런처의
 * "빈 포트인가" 검사는 통과하는데 준비 확인 요청은 남의 응답에 걸려, vite가 멀쩡한데도 기동이 실패한다.
 * 우리가 먼저 잡아 두면 런처가 이미 갖춰 둔 대체 포트 경로를 타므로 그 혼선이 아예 생기지 않는다.
 */
function reservePreferredDevServerPort() {
  const holder = net.createServer();
  holder.listen(PREFERRED_DEV_SERVER_PORT, "127.0.0.1");
  holder.on("error", () => {
    /** 이미 누가 IPv4로도 잡고 있으면 런처가 알아서 대체 포트를 고른다 */
  });
  return holder;
}

function startDesktop(runDir, appDataDir) {
  const logPath = path.join(runDir, "desktop.log");
  const logFile = fs.openSync(logPath, "a");

  const desktop = spawn("pnpm", ["dev"], {
    cwd: path.resolve(__dirname, "../../.."),
    env: { ...process.env, KANVIBE_APP_DATA_DIR: appDataDir },
    stdio: ["ignore", logFile, logFile],

    /** vite와 electron을 자식으로 더 낳으므로, 정리할 때 프로세스 그룹째 보내려면 그룹장이어야 한다 */
    detached: true,
  });

  return { desktop, logPath };
}

async function waitForDesktop() {
  const deadline = Date.now() + DESKTOP_READY_TIMEOUT_MS;
  let lastError = null;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${DESKTOP_API}/api/hooks/health`);
      if (response.ok) {
        return;
      }
      lastError = `HTTP ${response.status}`;
    } catch (error) {
      lastError = error.message;
    }
    await new Promise((resolve) => setTimeout(resolve, DESKTOP_POLL_INTERVAL_MS));
  }

  throw new Error(`데스크탑이 ${DESKTOP_READY_TIMEOUT_MS}ms 안에 열리지 않았습니다: ${lastError}`);
}

async function startHookTask(body) {
  const response = await fetch(`${DESKTOP_API}/api/hooks/start`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

  const payload = await response.json();
  if (!response.ok || !payload.success) {
    throw new Error(`픽스처 태스크 생성 실패: HTTP ${response.status} ${JSON.stringify(payload)}`);
  }
  return payload.data;
}

/**
 * 프로젝트는 데스크탑 화면에서만 등록할 수 있어 HTTP 경로가 없다. 격리 DB에 직접 한 줄 넣는다.
 *
 * 데스크탑이 첫 요청에서 DB를 만들어 두므로 이 시점에는 스키마가 이미 있다.
 */
function insertFixtureProject(appDataDir, repoPath) {
  const databasePath = path.join(appDataDir, "kanvibe.db");
  const escapedRepoPath = repoPath.replace(/'/g, "''");

  run("sqlite3", [
    databasePath,
    `INSERT INTO projects (id, name, repo_path, default_branch) VALUES ('${FIXTURE_PROJECT_ID}', '${FIXTURE_PROJECT_NAME}', '${escapedRepoPath}', 'main');`,
  ]);

  return FIXTURE_PROJECT_ID;
}

/**
 * 데스크탑은 worktree만 만들고 tmux 세션은 터미널을 여는 시점에 만든다(`src/lib/worktree.ts`).
 * 기기에서 비출 pane이 있으려면 여기서 세션을 직접 띄워야 한다.
 */
function startFixtureSession(sessionName, worktreePath) {
  run("tmux", ["new-session", "-d", "-s", sessionName, "-c", worktreePath]);
  run("tmux", ["send-keys", "-t", sessionName, `printf '${PANE_READY_MARKER}\\n'`, "Enter"]);
}

function capturePane(sessionName) {
  try {
    return run("tmux", ["capture-pane", "-p", "-t", sessionName]);
  } catch {
    return "";
  }
}

/**
 * 기기에서 친 키는 소켓을 타고 가 셸이 처리한 뒤에야 흔적을 남긴다. 한 번 보고 없다고 답하면
 * 아직 도착하지 않은 것을 도착하지 않을 것으로 단정하게 되므로 주어진 시간만큼 다시 본다.
 */
async function waitForMarkerFile(worktreePath, marker, timeoutMs) {
  const markerPath = path.join(worktreePath, marker);
  const deadline = Date.now() + timeoutMs;

  for (;;) {
    if (marker.length > 0 && fs.existsSync(markerPath)) {
      return true;
    }
    if (Date.now() >= deadline) {
      return false;
    }
    await new Promise((resolve) => setTimeout(resolve, MARKER_POLL_INTERVAL_MS));
  }
}

/**
 * 기기에서 친 명령을 셸이 실제로 실행했는지 플로우가 물어보는 창구.
 *
 * 기기 화면의 터미널은 캔버스로 그려져 접근성 트리에 글자가 없으므로 화면만으로는 단정할 수 없다.
 * pane에 찍힌 글자를 보는 것으로도 부족하다 — pty는 받은 키를 되비추므로, 셸이 실행하지 않아도
 * 친 글자는 그대로 보인다. 그래서 실행되어야만 생기는 파일을 본다. 함께 돌려주는 pane 내용은
 * 실패했을 때 무엇이 찍혔는지 보기 위한 것이다.
 */
function startCommandProbe(sessionName, worktreePath) {
  const server = http.createServer(async (request, response) => {
    const requestUrl = new URL(request.url, `http://127.0.0.1:${PROBE_PORT}`);

    if (requestUrl.pathname !== "/command-ran") {
      response.writeHead(404).end();
      return;
    }

    const marker = requestUrl.searchParams.get("marker") ?? "";
    const ran = await waitForMarkerFile(worktreePath, marker, Number(requestUrl.searchParams.get("timeoutMs") ?? 0));

    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ ran, pane: capturePane(sessionName) }));
  });

  server.listen(PROBE_PORT, "127.0.0.1");
  return server;
}

/**
 * 만든 것 중 run 디렉터리 밖에 사는 것만 되돌린다.
 *
 * tmux 세션은 머신 전역의 tmux 서버에 붙고 데스크탑은 프로세스 트리라 여기서 지워야 한다.
 * worktree와 픽스처 저장소는 run 디렉터리 안에 있어 디렉터리를 지우는 것으로 함께 사라진다.
 */
async function cleanUp({ desktop, sessionName }) {
  try {
    run("tmux", ["kill-session", "-t", sessionName]);
  } catch {
    /** 세션이 이미 없으면 지울 것도 없다 */
  }

  try {
    process.kill(-desktop.pid, "SIGTERM");
  } catch {
    /** 이미 죽은 프로세스 그룹 */
    return;
  }

  /**
   * SIGTERM만 보내고 끝내면 vite가 포트를 쥔 채 남아, 다음 실행의 데스크탑이 화면을 못 찾고 죽는다.
   * 그 실패는 다음 실행에서야 드러나므로 잠시 기다렸다가 확실히 끊는다.
   */
  await new Promise((resolve) => setTimeout(resolve, DESKTOP_SIGKILL_DELAY_MS));
  try {
    process.kill(-desktop.pid, "SIGKILL");
  } catch {
    /** SIGTERM으로 이미 정리됐다 */
  }
}

async function main() {
  const runDirIndex = process.argv.indexOf("--run-dir");
  if (runDirIndex === -1) {
    throw new Error("--run-dir <경로>가 필요합니다");
  }
  const runDir = path.resolve(process.argv[runDirIndex + 1]);
  const appDataDir = path.join(runDir, "app-data");
  fs.mkdirSync(appDataDir, { recursive: true });

  const repoPath = createFixtureRepository(runDir);
  const devPortHolder = reservePreferredDevServerPort();
  const { desktop, logPath } = startDesktop(runDir, appDataDir);

  try {
    await waitForDesktop();

    await startHookTask({ title: NO_SESSION_TASK_TITLE });

    const projectId = insertFixtureProject(appDataDir, repoPath);
    const sessionTask = await startHookTask({
      title: SESSION_TASK_TITLE,
      branchName: FIXTURE_BRANCH,
      sessionType: "tmux",
      projectId,
    });

    if (!sessionTask.sessionName) {
      throw new Error("세션 있는 픽스처 태스크에 sessionName이 붙지 않았습니다. 데스크탑 로그를 확인하세요: " + logPath);
    }

    const worktreePath = path.join(
      path.dirname(repoPath),
      `${path.basename(repoPath)}__worktrees`,
      FIXTURE_BRANCH.replace(/\//g, "-"),
    );
    startFixtureSession(sessionTask.sessionName, worktreePath);
    const probe = startCommandProbe(sessionTask.sessionName, worktreePath);

    const fixture = {
      appDataDir,
      repoPath,
      worktreePath,
      projectId,
      sessionName: sessionTask.sessionName,
      sessionTaskId: sessionTask.id,
      noSessionTaskTitle: NO_SESSION_TASK_TITLE,
      sessionTaskTitle: SESSION_TASK_TITLE,
      paneReadyMarker: PANE_READY_MARKER,
      probeUrl: `http://127.0.0.1:${PROBE_PORT}`,
      desktopLog: logPath,
    };
    fs.writeFileSync(path.join(runDir, "fixture.json"), `${JSON.stringify(fixture, null, 2)}\n`);
    console.log(JSON.stringify(fixture));

    const stop = () => {
      probe.close();
      devPortHolder.close();
      cleanUp({ desktop, sessionName: sessionTask.sessionName }).finally(() => process.exit(0));
    };
    process.on("SIGTERM", stop);
    process.on("SIGINT", stop);
  } catch (error) {
    devPortHolder.close();
    await cleanUp({ desktop, sessionName: "" });
    throw error;
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
