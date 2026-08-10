#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * docs용 태스크 상세 스크린샷 캡처 플로우.
 * 터미널 탭 바가 보이는 상태를 만들어 `task-detail.png`와 탭 바 근접 컷을 함께 남긴다.
 * 탭 이름을 역할별로 바꿔 두어 "탭을 무엇에 쓰는지"가 이미지만 봐도 읽히게 한다.
 */
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const ROOT_DIR = process.env.KANVIBE_ROOT_DIR;
const OUT_DIR = process.env.CAPTURE_OUT_DIR;
/** 교체 대상인 기존 task-detail.png와 같은 크기로 찍어 문서 레이아웃이 흔들리지 않게 한다 */
const DETAIL_VIEWPORT = { width: 1600, height: 1000 };

/**
 * 탭이 역할별로 갈린다는 것이 보이도록 기본 셸 이름 대신 실제로 쓸 법한 이름을 주고,
 * 각 탭에 그 역할다운 출력을 남겨 빈 터미널이 찍히지 않게 한다.
 * 셸이 다시 파싱하는 문자열이므로 작은따옴표는 쓰지 않는다.
 */
const TAB_FIXTURES = [
  {
    name: "claude",
    lines: [
      "> 터미널 탭 바를 tmux window와 동기화해줘",
      "",
      "  Search  tmux window (17 files)",
      "  Read    src/lib/terminal.ts (412 lines)",
      "  Read    src/lib/worktree.ts (688 lines)",
      "",
      "  세션 타입마다 탭 주인이 다릅니다. tmux/zellij는 멀티플렉서가 탭을 소유하고,",
      "  terminal 세션은 KanVibe가 PTY를 직접 관리합니다. 탭 조작을 한 서비스로 모으고",
      "  세션 타입별로 분기하겠습니다.",
      "",
      "  Write   src/lib/terminalTabs.ts",
      "  Write   src/desktop/main/services/terminalTabService.ts",
      "  Write   src/desktop/renderer/hooks/useTerminalTabs.ts",
      "  Edit    src/desktop/renderer/components/TerminalTabBar.tsx",
      "  Edit    src/desktop/renderer/routes/TaskDetailRoute.tsx",
      "",
      "  Bash    pnpm test -- terminalTabs terminalTabService",
      "          Test Files  3 passed (3)",
      "               Tests  61 passed (61)",
      "",
      "  탭 바를 붙이고 tmux window 목록을 1초 주기로 폴링해 동기화했습니다.",
      "  KanVibe에서 탭을 추가/삭제/이름변경하면 tmux window에 그대로 반영되고,",
      "  터미널에서 직접 tmux 명령을 써도 탭 바가 따라옵니다.",
      "",
      "  - 새 탭        Ctrl+T",
      "  - 탭 닫기      Ctrl+W (마지막 탭이면 창까지)",
      "  - 탭 이동      Ctrl+Shift+[ / ]",
      "  - 탭 직접 선택 Ctrl+Alt+1 ~ 5",
    ],
    /** 작업 정보 패널이 왼쪽을 가리므로, 읽혀야 할 요약은 오른쪽 pane에 둔다 */
    splitLines: [
      "터미널 탭 체크리스트",
      "",
      "[x] tmux window <-> KanVibe 탭 동기화",
      "[x] zellij tab <-> KanVibe 탭 동기화",
      "[x] terminal 세션은 KanVibe가 PTY 직접 관리",
      "[x] 드래그로 탭 순서 변경",
      "[x] 더블클릭으로 탭 이름 변경",
      "[ ] 문서에 터미널 탭 섹션 추가",
      "",
      "탭 바는 세션 타입을 가리지 않는다.",
      "tmux든 zellij든 순수 terminal이든",
      "같은 탭 바에서 같은 방식으로 다룬다.",
    ],
  },
  {
    name: "dev server",
    lines: ["  VITE v7.1.4  ready in 431 ms", "", "  ➜  Local:   http://localhost:5173/", "  ➜  press h + enter to show help"],
  },
  {
    name: "test",
    lines: [" Test Files  107 passed (107)", "      Tests  1020 passed (1020)", "   Duration  18.42s"],
  },
];

const PROJECT_NAME = "kanvibe";
const TASK_TITLE = "feat/terminal-tab";
const TASK_DESCRIPTION = "태스크 상세 터미널에 탭 바를 붙이고 tmux window와 동기화";

function execGit(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function execTmux(args) {
  return execFileSync("tmux", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 10_000 }).trim();
}

function createLocalRepository(fixturesRoot) {
  const projectDir = path.join(fixturesRoot, PROJECT_NAME);
  fs.mkdirSync(projectDir, { recursive: true });

  execGit(["init", "--initial-branch=main"], projectDir);
  execGit(["config", "user.email", "qa@kanvibe.local"], projectDir);
  execGit(["config", "user.name", "KanVibe QA"], projectDir);
  fs.writeFileSync(path.join(projectDir, "README.md"), `# ${PROJECT_NAME}\n`, "utf8");
  execGit(["add", "."], projectDir);
  execGit(["commit", "-m", "chore: init"], projectDir);
  execGit(["remote", "add", "origin", `https://github.com/rookedsysc/${PROJECT_NAME}.git`], projectDir);

  return projectDir;
}

/**
 * Playwright의 setViewportSize는 deviceScaleFactor를 1로 고정해 Retina 배율을 죽인다.
 * docs용 이미지는 2배 밀도가 필요하므로 Electron 쪽에서 직접 촬영한다.
 */
async function captureRetinaScreenshot(app, shotPath) {
  const encodedPng = await app.evaluate(async ({ BrowserWindow, screen }) => {
    const win = BrowserWindow.getAllWindows()[0];
    const image = await win.webContents.capturePage();
    return { png: image.toPNG().toString("base64"), scaleFactor: screen.getPrimaryDisplay().scaleFactor };
  });

  fs.writeFileSync(shotPath, Buffer.from(encodedPng.png, "base64"));
  return encodedPng.scaleFactor;
}

/**
 * 작업 정보 패널은 탭 바와 터미널 왼쪽을 덮으므로, 터미널 전체가 보여야 하는 컷에서는 먼저 닫는다.
 * 앱이 설계한 해제 방법 그대로 패널 밖 터미널 본문을 누른 뒤,
 * 탭 바 왼쪽 끝이 실제로 맨 앞에 나올 때까지 기다린다.
 */
async function dismissDetailPanel(page) {
  await page.mouse.click(Math.round(DETAIL_VIEWPORT.width * 0.75), Math.round(DETAIL_VIEWPORT.height * 0.6));

  await page.waitForFunction(() => {
    const tabBar = document.querySelector("[data-testid='terminal-tab-bar']");
    if (!tabBar) return false;
    const rect = tabBar.getBoundingClientRect();
    return tabBar.contains(document.elementFromPoint(rect.left + 8, rect.top + rect.height / 2));
  }, undefined, { timeout: 10000 });
}

/**
 * 사용량 패널을 dock 최하단 버튼으로 연다.
 *
 * 조회가 네트워크 왕복이라 카드가 값으로 채워질 때까지 기다린다.
 * 새로고침 버튼이 다시 눌릴 수 있게 되면 갱신이 끝난 것이므로, 그때 찍어야 "갱신 중"이 남지 않는다.
 */
async function openAiUsagePanel(page) {
  await page.getByTestId("task-detail-usage-button").click();

  await page.waitForFunction(() => {
    const panel = document.querySelector("[data-testid='ai-usage-panel']");
    const refreshButton = panel?.querySelector("[data-testid='ai-usage-refresh']");
    return Boolean(refreshButton)
      && !refreshButton.hasAttribute("disabled")
      && /\d+%/.test(panel.textContent || "");
  }, undefined, { timeout: 40000 });
}

/** 작업 정보 패널을 다시 띄운다. dock 첫 칸의 단축키가 그대로 토글이다 */
async function openDetailPanel(page) {
  await page.keyboard.press(process.platform === "darwin" ? "Meta+1" : "Control+1");
  await page.waitForFunction(() => {
    const tabBar = document.querySelector("[data-testid='terminal-tab-bar']");
    if (!tabBar) return false;
    const rect = tabBar.getBoundingClientRect();
    return !tabBar.contains(document.elementFromPoint(rect.left + 8, rect.top + rect.height / 2));
  }, undefined, { timeout: 10000 });
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
    { namespace, method, args },
  );
}

async function dismissDialogIfPresent(page) {
  try {
    await page.getByRole("button", { name: "닫기" }).first().click({ timeout: 2500 });
    await page.waitForTimeout(500);
  } catch {
    // 릴리스 노트 다이얼로그는 없을 수도 있다.
  }
}

async function readTabs(page) {
  return page.evaluate(() => (
    [...document.querySelectorAll("[data-terminal-tab-id]")].map((element) => ({
      id: element.getAttribute("data-terminal-tab-id"),
      name: element.querySelector("[data-terminal-tab-name]")?.textContent.trim() ?? "",
    }))
  ));
}

function printLinesInPane(paneTarget, lines) {
  const printfArguments = lines.map((line) => `'${line}'`).join(" ");
  execTmux([
    "send-keys",
    "-t",
    paneTarget,
    `clear; printf '%s\\n' ${printfArguments}; while :; do sleep 3600; done`,
    "Enter",
  ]);
}

/** 스크린샷에 빈 프롬프트만 남지 않도록 각 window에 역할다운 출력을 찍어 둔다 */
function seedWindowContent(windowId, fixture) {
  printLinesInPane(windowId, fixture.lines);

  if (fixture.splitLines) {
    execTmux(["split-window", "-h", "-t", windowId]);
    printLinesInPane(`${windowId}.1`, fixture.splitLines);
  }
}

/** 문서가 설명하는 조작 그대로 더블클릭 인라인 편집으로 이름을 바꾼다 */
async function renameTabThroughUi(page, tabId, nextName) {
  const tab = page.locator(`[data-terminal-tab-id='${tabId}']`);
  await tab.dblclick();

  const renameInput = page.getByTestId("terminal-tab-rename-input");
  await renameInput.fill(nextName);
  await renameInput.press("Enter");

  await page.waitForFunction(
    ({ tabId, nextName }) => (
      document.querySelector(`[data-terminal-tab-id='${tabId}'] [data-terminal-tab-name]`)?.textContent.trim() === nextName
    ),
    { tabId, nextName },
    { timeout: 20000 },
  );
}

async function waitForTabCount(page, expectedCount) {
  await page.waitForFunction(
    (count) => document.querySelectorAll("[data-terminal-tab-id]").length === count,
    expectedCount,
    { timeout: 20000 },
  );
}

async function main() {
  const fixturesRoot = path.join(OUT_DIR, "fixtures");
  const appDataDir = path.join(OUT_DIR, "app-data");
  fs.rmSync(fixturesRoot, { recursive: true, force: true });
  fs.rmSync(appDataDir, { recursive: true, force: true });
  fs.mkdirSync(fixturesRoot, { recursive: true });
  fs.mkdirSync(appDataDir, { recursive: true });

  const projectDir = createLocalRepository(fixturesRoot);
  const { app, page } = await launchKanVibeElectron({
    rootDir: ROOT_DIR,
    appDataDir,
    viewport: DETAIL_VIEWPORT,
    timeoutMs: 60000,
    actionTimeoutMs: 30000,
  });

  let sessionName = null;

  try {
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(2500);
    await dismissDialogIfPresent(page);

    const projectResult = await invokeDesktop(page, "project", "registerProject", PROJECT_NAME, projectDir);
    if (!projectResult?.success) throw new Error(projectResult?.error || "프로젝트 등록 실패");

    const rootTaskId = await invokeDesktop(page, "kanban", "getTaskIdByProjectAndBranch", projectResult.project.id, "main");
    const task = await invokeDesktop(page, "kanban", "createTask", {
      title: TASK_TITLE,
      description: TASK_DESCRIPTION,
      projectId: projectResult.project.id,
      baseBranch: "main",
    });
    await invokeDesktop(page, "kanban", "moveTaskToColumn", task.id, "progress", [task.id]);

    const connected = await invokeDesktop(page, "kanban", "connectTerminalSession", task.id, "tmux");
    if (!connected?.sessionName) throw new Error("tmux 세션 연결 실패");
    sessionName = connected.sessionName;
    console.log(`[capture] root task=${rootTaskId}, detail task=${task.id}, session=${sessionName}`);

    await page.evaluate((id) => { window.location.hash = `#/ko/task/${id}`; }, task.id);
    await page.getByTestId("terminal-tab-bar").waitFor({ state: "visible", timeout: 30000 });
    await page.waitForTimeout(2500);
    /** 기본으로 열리는 작업 정보 패널은 탭 바를 덮으므로, 탭을 만들기 전에 먼저 걷는다 */
    await dismissDetailPanel(page);

    for (let tabIndex = 1; tabIndex < TAB_FIXTURES.length; tabIndex += 1) {
      await page.getByTestId("terminal-tab-new").click();
      await waitForTabCount(page, tabIndex + 1);
    }

    const tabs = await readTabs(page);
    for (const [tabIndex, tab] of tabs.entries()) {
      seedWindowContent(tab.id, TAB_FIXTURES[tabIndex]);
      await renameTabThroughUi(page, tab.id, TAB_FIXTURES[tabIndex].name);
    }

    /** 첫 탭을 활성으로 되돌려 "여러 탭 중 하나를 보고 있다"가 드러나게 한다 */
    await page.locator(`[data-terminal-tab-id='${tabs[0].id}']`).click();
    /** 탭 위에 커서가 남으면 단축키 힌트가 닫기 버튼으로 바뀐 채 찍힌다 */
    await page.mouse.move(Math.round(DETAIL_VIEWPORT.width * 0.5), Math.round(DETAIL_VIEWPORT.height * 0.8));
    await page.waitForTimeout(2000);

    /** 탭 설명용 컷은 탭 바와 그 아래 터미널이 한 화면에 다 들어와야 하므로 패널이 없는 상태로 찍는다 */
    const tabsPath = path.join(OUT_DIR, "terminal-tabs.png");
    const scaleFactor = await captureRetinaScreenshot(app, tabsPath);

    /** 워크스페이스 컷은 작업 정보 패널을 다시 띄운 실제 기본 모습으로 찍는다 */
    await openDetailPanel(page);
    await page.waitForTimeout(1500);

    const detailPath = path.join(OUT_DIR, "task-detail.png");
    await captureRetinaScreenshot(app, detailPath);

    /** 사용량 컷은 같은 화면에서 dock 최하단 버튼을 눌러 실제 사용 경로 그대로 찍는다 */
    await openAiUsagePanel(page);
    /** 버튼 위에 커서가 남으면 hover 상태로 찍힌다 */
    await page.mouse.move(Math.round(DETAIL_VIEWPORT.width * 0.75), Math.round(DETAIL_VIEWPORT.height * 0.6));
    await page.waitForTimeout(1500);

    const usagePath = path.join(OUT_DIR, "ai-usage.png");
    await captureRetinaScreenshot(app, usagePath);

    console.log(JSON.stringify({
      ok: true,
      scaleFactor,
      tabs: (await readTabs(page)).map((tab) => tab.name),
      screenshots: [detailPath, tabsPath, usagePath],
    }));
  } catch (error) {
    console.error("[capture] electron output:\n" + (typeof app.output === "function" ? app.output() : ""));
    throw error;
  } finally {
    /** 멀티플렉서 서버가 앱의 PTY stdio를 물고 있으면 app.close()가 반환하지 않는다 */
    if (sessionName) {
      try {
        execTmux(["kill-session", "-t", sessionName]);
      } catch {
        // 세션이 이미 사라진 경우 무시
      }
    }
    await app.close().catch(() => {});
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
