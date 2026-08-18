#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * docs용 AI 계정 화면 스크린샷 캡처 플로우.
 *
 * 이 컷이 증명해야 하는 것은 "등록한 계정마다 로그인 상태와 구독 등급이 보이고,
 * 남은 사용량이 같은 화면에 함께 선다"이다. 그래서 실제 로컬 로그인 상태를 그대로 찍고,
 * 계정 이메일만 예시 주소로 가린다.
 */
const fs = require("node:fs");
const path = require("node:path");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");
const { maskAccountLabels } = require("../lib/accountLabelMask.cjs");

const ROOT_DIR = process.env.KANVIBE_ROOT_DIR;
const OUT_DIR = process.env.CAPTURE_OUT_DIR;

/**
 * 계정 목록은 가운데 정렬된 좁은 단이라 폭이 넓으면 여백만 찍힌다.
 * 세 provider 구획과 사용량 막대가 스크롤 없이 한 화면에 들어오는 높이를 준다.
 */
const ACCOUNTS_VIEWPORT = { width: 1100, height: 1210 };

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
 * 릴리스 노트 다이얼로그가 떠 있으면 걷는다.
 *
 * 다이얼로그는 라우트가 뜬 뒤 한 박자 늦게 나타나므로 늦게 한 번 부르는 편이 항상 낫다.
 * 남은 backdrop은 전체 화면을 덮어 이후 모든 조작을 삼킨다.
 */
async function dismissDialogIfPresent(page) {
  const dialog = page.getByRole("dialog");
  try {
    await dialog.getByRole("button", { name: "닫기" }).click({ timeout: 2500 });
  } catch {
    return;
  }

  await dialog.waitFor({ state: "hidden", timeout: 10000 });
}

/**
 * 계정 목록과 사용량 조회가 모두 끝날 때까지 기다린다.
 *
 * 계정 목록은 provider CLI를 자식 프로세스로 부르고 사용량은 네트워크를 타므로 둘 다 느리다.
 * 새로고침 버튼이 다시 눌릴 수 있게 되면 갱신이 끝난 것이라, 그때 찍어야 "갱신 중"이 남지 않는다.
 */
async function waitForAccountsSettled(page) {
  await page.waitForFunction(() => {
    const route = document.querySelector("[data-testid='ai-accounts-route']");
    if (!route) return false;

    const hasAccountRow = Boolean(route.querySelector("[data-testid='ai-account-row']"));
    const refreshButton = route.querySelector("[data-testid='ai-usage-refresh']");
    const usagePanel = route.querySelector("[data-testid='ai-usage-panel']");
    return hasAccountRow
      && Boolean(refreshButton)
      && !refreshButton.hasAttribute("disabled")
      && /\d+%/.test(usagePanel?.textContent || "");
  }, undefined, { timeout: 60000 });
}

async function main() {
  const appDataDir = path.join(OUT_DIR, "app-data");
  fs.rmSync(appDataDir, { recursive: true, force: true });
  fs.mkdirSync(appDataDir, { recursive: true });

  const { app, page } = await launchKanVibeElectron({
    rootDir: ROOT_DIR,
    appDataDir,
    viewport: ACCOUNTS_VIEWPORT,
    timeoutMs: 60000,
    actionTimeoutMs: 30000,
  });

  try {
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(2500);

    await page.evaluate(() => { window.location.hash = "#/ko/ai-accounts"; });
    await page.getByTestId("ai-accounts-route").waitFor({ state: "visible", timeout: 30000 });
    await dismissDialogIfPresent(page);

    await waitForAccountsSettled(page);
    const maskedLabelCount = await maskAccountLabels(page, [
      "ai-account-label",
      "ai-usage-account-label",
    ]);

    /** 버튼 위에 커서가 남으면 hover 상태로 찍힌다 */
    await page.mouse.move(
      Math.round(ACCOUNTS_VIEWPORT.width * 0.08),
      Math.round(ACCOUNTS_VIEWPORT.height * 0.95),
    );
    await page.waitForTimeout(1500);

    const accountsPath = path.join(OUT_DIR, "ai-accounts.png");
    const scaleFactor = await captureRetinaScreenshot(app, accountsPath);

    const providerSections = await page.evaluate(() => (
      [...document.querySelectorAll("[data-testid^='ai-accounts-provider-']")]
        .map((section) => section.getAttribute("data-testid").replace("ai-accounts-provider-", ""))
    ));

    console.log(JSON.stringify({
      ok: true,
      scaleFactor,
      maskedLabelCount,
      providerSections,
      screenshots: [accountsPath],
    }));
  } catch (error) {
    console.error("[capture] electron output:\n" + (typeof app.output === "function" ? app.output() : ""));
    throw error;
  } finally {
    await app.close().catch(() => {});
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
