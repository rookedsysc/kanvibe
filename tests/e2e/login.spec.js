const { test, expect } = require("@playwright/test");

test.describe("login e2e", () => {
  test("logs in through the actual form button", async ({ page }) => {
    await page.goto("/ko/login");

    await expect(page.getByLabel("사용자 이름")).toBeVisible();

    await page.getByLabel("사용자 이름").fill("admin");
    await page.getByLabel("비밀번호").fill("changeme");
    await page.getByRole("button", { name: "로그인" }).click();

    await page.waitForURL((url) => !url.pathname.endsWith("/login"));
    await expect(page.getByRole("button", { name: "+ 새 작업" })).toBeVisible();
    await expect(page.getByRole("button", { name: "로그아웃" })).toBeVisible();
  });
});
