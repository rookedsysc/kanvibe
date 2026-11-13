import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AiChatAttachment } from "../AiChatAttachment";
import { AiChatTranscriptMessage } from "../AiChatTranscriptMessage";
import type { AiSessionAttachmentContent } from "@/lib/aiSessions/types";

const api = vi.hoisted(() => ({ getTaskAiSessionAttachment: vi.fn() }));
vi.mock("@/desktop/renderer/actions/project", () => api);
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@/components/AiProviderIcon", () => ({ AiProviderIcon: () => <span>Codex</span> }));
vi.mock("../AiSessionMessageContent", () => ({ AiSessionMessageContent: ({ text }: { text: string }) => <p>{text}</p> }));

const image = { id: "image-a", kind: "image" as const, name: "photo.png", mimeType: "image/png", sizeBytes: 68 };
const context = { taskId: "task-a", provider: "codex" as const, sessionId: "session-a", sourceRef: "/owned/rollout.jsonl" };
const dataUrl = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aRZkAAAAASUVORK5CYII=";
const content = { id: image.id, name: image.name, mimeType: image.mimeType, dataUrl };

beforeEach(() => { api.getTaskAiSessionAttachment.mockReset(); });
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe("authenticated history attachments", () => {
  it("renders an image-only message and expands its image without exposing an empty copy action", async () => {
    api.getTaskAiSessionAttachment.mockResolvedValue(content);
    render(<AiChatTranscriptMessage provider="codex" attachmentContext={context} message={{ role: "user", text: "", fullText: "", timestamp: null, isTruncated: false, attachments: [image] }} />);
    expect(await screen.findByAltText("photo.png")).toBeTruthy();
    expect(api.getTaskAiSessionAttachment).toHaveBeenCalledWith("task-a", "codex", "session-a", "image-a", "/owned/rollout.jsonl");
    expect(screen.queryByRole("button", { name: "aiSessions.copyMessage" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "openImage" }));
    expect(screen.getByRole("button", { name: "closeImage" }).getAttribute("aria-expanded")).toBe("true");
  });

  it("keeps late media responses from the previous task out of the current image", async () => {
    let resolveOld!: (value: AiSessionAttachmentContent) => void;
    api.getTaskAiSessionAttachment.mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve; }));
    const view = render(<AiChatAttachment attachment={image} context={context} />);
    await waitFor(() => expect(api.getTaskAiSessionAttachment).toHaveBeenCalledTimes(1));
    const next = { ...image, id: "image-b", name: "current.png" };
    api.getTaskAiSessionAttachment.mockResolvedValue({ ...content, id: next.id, name: next.name });
    view.rerender(<AiChatAttachment attachment={next} context={{ ...context, taskId: "task-b", sessionId: "session-b" }} />);
    await screen.findByAltText("current.png");
    await act(async () => resolveOld({ ...content, dataUrl: "data:image/png;base64,T0xEX1JFU1BPTlNF" }));
    expect(screen.getByAltText("current.png").getAttribute("src")).toBe(dataUrl);
    expect(screen.queryByAltText("photo.png")).toBeNull();
  });

  it("shows an unavailable attachment and recovers when the user retries", async () => {
    api.getTaskAiSessionAttachment.mockResolvedValueOnce(null).mockResolvedValueOnce(content);
    render(<AiChatAttachment attachment={image} context={context} />);
    await screen.findByText("attachmentUnavailable");
    fireEvent.click(screen.getByRole("button", { name: "retry" }));
    await screen.findByAltText("photo.png");
    expect(screen.queryByText("attachmentUnavailable")).toBeNull();
  });

  it("refuses file URLs and active image markup returned by the media boundary", async () => {
    api.getTaskAiSessionAttachment.mockResolvedValue({ ...content, dataUrl: "file:///owned/private.png" });
    const view = render(<AiChatAttachment attachment={image} context={context} />);
    await screen.findByText("attachmentUnavailable");
    expect(screen.queryByRole("img")).toBeNull();
    api.getTaskAiSessionAttachment.mockResolvedValue({ ...content, dataUrl: "data:image/svg+xml;base64,PHN2Zy8+" });
    fireEvent.click(screen.getByRole("button", { name: "retry" }));
    await waitFor(() => expect(api.getTaskAiSessionAttachment).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("img")).toBeNull();
    view.unmount();
  });

  it("keeps file bytes lazy until an explicit download", () => {
    render(<AiChatAttachment attachment={{ ...image, kind: "file", name: "notes.txt", mimeType: "text/plain" }} context={context} />);
    expect(screen.getByRole("button", { name: "downloadAttachment: notes.txt" })).toBeTruthy();
    expect(api.getTaskAiSessionAttachment).not.toHaveBeenCalled();
  });
});
