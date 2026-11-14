import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AiAccountsRoute from "@/desktop/renderer/routes/AiAccountsRoute";
import { addAiAccount, listAiAccounts, removeAiAccount } from "@/desktop/renderer/actions/aiAccounts";
import type { AiAccountSummary } from "@/desktop/main/services/aiAccountService";

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

vi.mock("@/desktop/renderer/components/AiUsagePanel", () => ({
  default: () => <div />,
}));

vi.mock("@/desktop/renderer/components/AiAccountLoginTerminal", () => ({
  default: () => <div />,
}));

function createAccount(overrides: Partial<AiAccountSummary> = {}): AiAccountSummary {
  return {
    provider: "claude",
    accountRoot: "/accounts/work",
    accountName: "work",
    label: "work",
    isLoggedIn: true,
    planName: null,
    isRemovable: true,
    ...overrides,
  };
}

describe("AiAccountsRoute - 요청 상태", () => {
  let confirmMock: ReturnType<typeof vi.spyOn>;
  let consoleErrorMock: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.mocked(listAiAccounts).mockReset();
    vi.mocked(addAiAccount).mockReset();
    vi.mocked(removeAiAccount).mockReset();
    confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    consoleErrorMock = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    confirmMock.mockRestore();
    consoleErrorMock.mockRestore();
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
