import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AiChatTranscriptMessage } from "../AiChatTranscriptMessage";

vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@/components/AiProviderIcon", () => ({ AiProviderIcon: () => <span>Claude</span> }));
vi.mock("../AiSessionMessageContent", () => ({ AiSessionMessageContent: ({ text }: { text: string }) => <p>{text}</p> }));

const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
afterEach(() => {
  if (clipboardDescriptor) Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
  else Reflect.deleteProperty(navigator, "clipboard");
});

describe("AiChatTranscriptMessage", () => {
  it("keeps tool output folded until its disclosure is opened and exposes its full output", () => {
    const { container } = render(<AiChatTranscriptMessage provider="claude" message={{ role: "tool", timestamp: null, text: "short", fullText: "Full tool output", isTruncated: true }} />);
    const disclosure = container.querySelector("details");
    expect(disclosure?.open).toBe(false);
    fireEvent.click(disclosure!.querySelector("summary")!);
    expect(disclosure?.open).toBe(true);
    expect(disclosure?.querySelector("p")?.textContent).toBe("Full tool output");
  });

  it("copies the full original message and reports success", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(<AiChatTranscriptMessage provider="claude" message={{ role: "assistant", timestamp: "2026-11-12T03:00:00Z", text: "preview", fullText: "Full original answer", isTruncated: true }} />);
    fireEvent.click(screen.getByRole("button", { name: "aiSessions.copyMessage" }));
    expect(await screen.findByRole("button", { name: "aiSessions.copiedMessage" })).toBeTruthy();
    expect(writeText).toHaveBeenCalledWith("Full original answer");
    expect(document.querySelector("time")?.dateTime).toBe("2026-11-12T03:00:00Z");
  });

  it("keeps the message available after clipboard failure", async () => {
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) } });
    render(<AiChatTranscriptMessage provider="claude" message={{ role: "user", timestamp: "invalid", text: "Keep this request", fullText: "Keep this request", isTruncated: false }} />);
    fireEvent.click(screen.getByRole("button", { name: "aiSessions.copyMessage" }));
    expect(await screen.findByRole("button", { name: "aiSessions.copyFailed" })).toBeTruthy();
    expect(screen.getByText("Keep this request")).toBeTruthy();
    expect(document.querySelector("time")).toBeNull();
  });

  it("사용자 요청은 마크다운으로 해석하지 않고 줄바꿈까지 입력한 그대로 보인다", () => {
    render(<AiChatTranscriptMessage provider="claude" message={{ role: "user", timestamp: null, text: "# 제목 아님\n둘째 줄", fullText: "# 제목 아님\n둘째 줄", isTruncated: false }} />);

    const request = screen.getByText(/# 제목 아님/);
    expect(request.textContent).toBe("# 제목 아님\n둘째 줄");
    expect(request.className).toContain("whitespace-pre-wrap");
  });

  it("도구 줄은 도구 이름을 보이고 이름이 없으면 역할 이름을 보인다", () => {
    const { container, rerender } = render(<AiChatTranscriptMessage provider="claude" message={{ role: "tool", toolName: "Bash", timestamp: null, text: "pnpm test", fullText: "pnpm test", isTruncated: false }} />);
    expect(container.querySelector("summary")?.textContent).toBe("Bashpnpm test");

    rerender(<AiChatTranscriptMessage provider="claude" message={{ role: "tool", timestamp: null, text: "output", fullText: "output", isTruncated: false }} />);
    expect(container.querySelector("summary")?.textContent).toBe("aiSessions.roles.tooloutput");
  });

  it("AI가 바꾼 파일마다 카드를 보이고 누르면 그 파일의 미리보기를 요청한다", () => {
    const onOpenFile = vi.fn();
    const files = [{ path: "/work/docs/plan.md", change: "created" as const }, { path: "/work/src/app.ts", change: "modified" as const }];
    const { container } = render(<AiChatTranscriptMessage provider="claude" onOpenFile={onOpenFile} message={{ role: "tool", toolName: "Write", timestamp: null, text: "/work/docs/plan.md", fullText: "/work/docs/plan.md", isTruncated: false, files }} />);

    const cards = screen.getAllByRole("button").filter((button) => button.title);
    expect(cards.map((card) => card.textContent)).toEqual(["plan.mdaiSessions.fileChange.created", "app.tsaiSessions.fileChange.modified"]);
    /** 카드는 접힌 도구 줄 밖에 있어야 펼치지 않고도 보인다 */
    expect(container.querySelector("details")?.contains(cards[0])).toBe(false);

    fireEvent.click(cards[1]);
    expect(onOpenFile).toHaveBeenCalledWith(files[1]);
  });

  it("미리보기를 열 수 없는 화면에서는 파일 카드를 그리지 않는다", () => {
    render(<AiChatTranscriptMessage provider="claude" message={{ role: "tool", toolName: "Write", timestamp: null, text: "x", fullText: "x", isTruncated: false, files: [{ path: "/work/a.md", change: "created" }] }} />);

    expect(screen.queryByText("a.md")).toBeNull();
  });

  it("shows system and developer messages directly instead of folding them", () => {
    const { container } = render(<AiChatTranscriptMessage provider="claude" message={{ role: "system", timestamp: null, text: "System instructions", fullText: "System instructions", isTruncated: false }} />);
    expect(container.querySelector("details")).toBeNull();
    expect(screen.getByText("System instructions")).toBeTruthy();
    expect(screen.getByText("aiSessions.roles.system")).toBeTruthy();
  });

});
