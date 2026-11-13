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

  it("shows system and developer messages directly instead of folding them", () => {
    const { container } = render(<AiChatTranscriptMessage provider="claude" message={{ role: "system", timestamp: null, text: "System instructions", fullText: "System instructions", isTruncated: false }} />);
    expect(container.querySelector("details")).toBeNull();
    expect(screen.getByText("System instructions")).toBeTruthy();
    expect(screen.getByText("aiSessions.roles.system")).toBeTruthy();
  });

});
