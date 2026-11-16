import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AiAccountsRoute from "@/desktop/renderer/routes/AiAccountsRoute";
import { addAiAccount, listAiAccounts, removeAiAccount } from "@/desktop/renderer/actions/aiAccounts";
import type { AiAccountSummary } from "@/desktop/main/services/aiAccountService";
import { getAiUsageSnapshot, getCachedAiUsageSnapshot } from "@/desktop/renderer/actions/aiUsage";
import type { AiUsageFailureReason, AiUsageSnapshot } from "@/lib/aiUsage/types";

vi.mock("@/desktop/renderer/actions/aiUsage", () => ({
  getAiUsageSnapshot: vi.fn(),
  getCachedAiUsageSnapshot: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/aiAccounts", () => ({
  listAiAccounts: vi.fn(),
  addAiAccount: vi.fn(),
  removeAiAccount: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children, href }: { children: React.ReactNode; href: string }) => <a href={href}>{children}</a>,
}));

vi.mock("@/components/AiProviderIcon", () => ({
  AiProviderIcon: () => <span />,
}));

vi.mock("@/desktop/renderer/components/AiAccountLoginTerminal", () => ({
  default: ({ provider, accountRoot, onExit }: { provider: string; accountRoot: string; onExit: (code: number) => void }) => (
    <div><button data-testid="login-terminal" onClick={() => onExit(0)}>{provider}:{accountRoot}</button><button onClick={() => onExit(1)}>fail-login</button></div>
  ),
}));

function createAccount(overrides: Partial<AiAccountSummary> = {}): AiAccountSummary {
  return {
    provider: "claude",
    accountId: "work-id",
    cachedAccountId: "cached-work-id",
    accountRoot: "/accounts/work",
    accountName: "work",
    label: "work",
    isLoggedIn: true,
    planName: null,
    isRemovable: true,
    ...overrides,
  };
}

function createUsage(
  accountId = "work-id",
  reason: AiUsageFailureReason = "expired-credentials",
): AiUsageSnapshot {
  return {
    fetchedAt: "2026-11-15T10:00:00Z",
    accounts: [{
      provider: "claude", accountId, label: "same-label", status: "unavailable",
      planName: null, windows: [], reason, fetchedAt: "2026-11-15T10:00:00Z",
    }],
  };
}

describe("AiAccountsRoute - 요청 상태", () => {
  let confirmMock: ReturnType<typeof vi.spyOn>;
  let consoleErrorMock: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.mocked(listAiAccounts).mockReset();
    vi.mocked(addAiAccount).mockReset();
    vi.mocked(removeAiAccount).mockReset();
    vi.mocked(getAiUsageSnapshot).mockReset().mockResolvedValue(createUsage());
    vi.mocked(getCachedAiUsageSnapshot).mockReset().mockResolvedValue(null);
    confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    consoleErrorMock = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    confirmMock.mockRestore();
    consoleErrorMock.mockRestore();
  });

  it("사용량 카드 안에서 만료 계정에 로그인하고 완료 후 계정과 사용량을 갱신한다", async () => {
    vi.mocked(listAiAccounts).mockResolvedValue([createAccount()]);
    render(<AiAccountsRoute />);
    const panel = await screen.findByTestId("ai-usage-panel");
    const row = within(panel).getByTestId("ai-account-row");
    expect(within(row).getByText("reasons.expired-credentials")).toBeTruthy();
    expect(within(row).queryByTestId("ai-usage-manage-accounts")).toBeNull();
    fireEvent.click(within(row).getByRole("button", { name: "login" }));
    expect(screen.getByTestId("login-terminal").textContent).toBe("claude:/accounts/work");
    fireEvent.click(screen.getByTestId("login-terminal"));
    await waitFor(() => expect(getAiUsageSnapshot).toHaveBeenCalledTimes(2));
    expect(listAiAccounts).toHaveBeenCalledTimes(2);
    expect(screen.queryByTestId("login-terminal")).toBeNull();
    expect(screen.getAllByTestId("ai-accounts-provider-claude")).toHaveLength(1);
  });

  it("Claude 실행을 기다리는 계정은 로그인된 계정으로 보여준다", async () => {
    vi.mocked(listAiAccounts).mockResolvedValue([createAccount()]);
    vi.mocked(getAiUsageSnapshot).mockResolvedValue(createUsage("work-id", "awaiting-cli-refresh"));
    render(<AiAccountsRoute />);
    const row = await screen.findByTestId("ai-account-row");
    await within(row).findByText("reasons.awaiting-cli-refresh");
    expect(within(row).getByText("loggedIn")).toBeTruthy();
    expect(within(row).getByRole("button", { name: "relogin" })).toBeTruthy();
  });

  it("로그인 명령 실패 시 터미널 출력을 남기고 닫은 뒤 다시 로그인할 수 있다", async () => {
    vi.mocked(listAiAccounts).mockResolvedValue([createAccount()]);
    render(<AiAccountsRoute />);
    fireEvent.click(await screen.findByTestId("ai-account-login"));
    fireEvent.click(screen.getByText("fail-login"));
    expect(screen.getByRole("alert").textContent).toContain('loginFailed:{"code":1}');
    expect(screen.getByTestId("login-terminal")).toBeTruthy();
    fireEvent.click(screen.getByTestId("ai-account-login-close"));
    expect(screen.queryByTestId("login-terminal")).toBeNull();
    fireEvent.click(screen.getByTestId("ai-account-login"));
    expect(screen.getByTestId("login-terminal")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("같은 이름의 계정도 ID로 구분하고 캐시 사용량을 해당 행에만 붙인다", async () => {
    vi.mocked(listAiAccounts).mockResolvedValue([
      createAccount({ label: "same-label" }),
      createAccount({ label: "same-label", accountRoot: "/accounts/home", accountId: "home-id", cachedAccountId: "cached-home-id" }),
    ]);
    vi.mocked(getAiUsageSnapshot).mockReturnValue(new Promise(() => {}));
    vi.mocked(getCachedAiUsageSnapshot).mockResolvedValue(createUsage("cached-home-id"));
    render(<AiAccountsRoute />);
    await screen.findByText("reasons.expired-credentials");
    const rows = screen.getAllByTestId("ai-account-row");
    expect(within(rows[0]).queryByText("reasons.expired-credentials")).toBeNull();
    expect(within(rows[1]).getByText("reasons.expired-credentials")).toBeTruthy();
    fireEvent.click(within(rows[1]).getByTestId("ai-account-login"));
    expect(screen.getByTestId("login-terminal").textContent).toBe("claude:/accounts/home");
  });

  it("사용량 조회 실패에도 카드 안에서 계정을 추가하고 로그인할 수 있다", async () => {
    vi.mocked(listAiAccounts).mockResolvedValue([]);
    vi.mocked(getAiUsageSnapshot).mockRejectedValue(new Error("offline"));
    vi.mocked(addAiAccount).mockResolvedValue({ outcome: "ok", accountRoot: "/accounts/new" });
    render(<AiAccountsRoute />);
    const panel = await screen.findByTestId("ai-usage-panel");
    const provider = within(panel).getByTestId("ai-accounts-provider-claude");
    fireEvent.change(within(provider).getByTestId("ai-account-name-input"), { target: { value: "new" } });
    fireEvent.click(within(provider).getByTestId("ai-account-add"));
    await waitFor(() => expect(screen.getByTestId("login-terminal").textContent).toBe("claude:/accounts/new"));
    expect(within(panel).getByText("snapshotFailed")).toBeTruthy();
  });

  it("처음 목록을 읽지 못하면 계정 없음 대신 사유와 다시 시도를 보이고 다시 읽는다", async () => {
    vi.mocked(listAiAccounts)
      .mockRejectedValueOnce(new Error("ipc down"))
      .mockResolvedValueOnce([createAccount()]);
    render(<AiAccountsRoute />);

    expect((await screen.findByRole("alert")).textContent).toContain('loadFailed:{"reason":"ipc down"}');
    expect(screen.queryByText("noAccounts")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "retry" }));

    expect(await screen.findByText("work")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("계정을 지우는 동안 그 행에 진행을 보이고 다른 삭제를 막으며, 실패하면 그 행에 사유를 남긴다", async () => {
    vi.mocked(listAiAccounts).mockResolvedValue([
      createAccount(),
      createAccount({ accountRoot: "/accounts/home", accountName: "home", label: "home" }),
    ]);
    let rejectRemove: (error: Error) => void = () => {};
    vi.mocked(removeAiAccount).mockReturnValueOnce(new Promise((_, reject) => {
      rejectRemove = reject;
    }));
    render(<AiAccountsRoute />);
    await screen.findByText("work");

    const [workRemove, homeRemove] = screen.getAllByTestId("ai-account-remove") as HTMLButtonElement[];
    fireEvent.click(workRemove);

    expect(workRemove.textContent).toBe("removing");
    expect(homeRemove.disabled).toBe(true);

    await act(async () => {
      rejectRemove(new Error("busy"));
    });

    const [workRow] = screen.getAllByTestId("ai-account-row");
    expect(workRow.querySelector("[role=alert]")?.textContent).toContain('removeFailed:{"reason":"busy"}');
    expect(removeAiAccount).toHaveBeenCalledTimes(1);
  });

  it("계정 추가 요청을 기다리는 동안 버튼을 막고, 요청이 실패하면 사유를 남긴다", async () => {
    vi.mocked(listAiAccounts).mockResolvedValue([]);
    let rejectAdd: (error: Error) => void = () => {};
    vi.mocked(addAiAccount).mockReturnValueOnce(new Promise((_, reject) => {
      rejectAdd = reject;
    }));
    render(<AiAccountsRoute />);

    const [claudeSection] = await screen.findAllByTestId("ai-account-name-input");
    fireEvent.change(claudeSection, { target: { value: "work" } });
    const [addButton] = screen.getAllByTestId("ai-account-add") as HTMLButtonElement[];
    fireEvent.click(addButton);
    fireEvent.click(addButton);

    expect(addButton.disabled).toBe(true);
    expect(addAiAccount).toHaveBeenCalledTimes(1);

    await act(async () => {
      rejectAdd(new Error("disk full"));
    });

    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain('addFailed:{"reason":"disk full"}');
    });
    expect(addButton.disabled).toBe(false);
  });
});
