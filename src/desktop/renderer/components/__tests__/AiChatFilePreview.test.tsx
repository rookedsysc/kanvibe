import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AiChatFilePreview, buildSandboxedPreviewDocument } from "../AiChatFilePreview";

const { readTaskWorkspaceFile } = vi.hoisted(() => ({ readTaskWorkspaceFile: vi.fn() }));
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@/desktop/renderer/actions/diff", () => ({ readTaskWorkspaceFile }));

const POLICY = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; form-action 'none'">`;
const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
const respondWith = (content: string) => readTaskWorkspaceFile.mockResolvedValue({ ok: true, file: { path: "docs/plan.md", content } });

beforeEach(() => { readTaskWorkspaceFile.mockReset(); });
afterEach(() => {
  if (clipboardDescriptor) Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
  else Reflect.deleteProperty(navigator, "clipboard");
});

describe("buildSandboxedPreviewDocument", () => {
  it("doctype이 있으면 그 바로 뒤에, 없으면 맨 앞에 요청 차단 정책을 넣는다", () => {
    expect(buildSandboxedPreviewDocument("<!DOCTYPE html><html><script>run()</script></html>")).toBe(`<!DOCTYPE html>${POLICY}<html><script>run()</script></html>`);
    expect(buildSandboxedPreviewDocument("\n <!doctype html>\n<p>a</p>")).toBe(`\n <!doctype html>${POLICY}\n<p>a</p>`);
    expect(buildSandboxedPreviewDocument("<script>run()</script><p>a</p>")).toBe(`${POLICY}<script>run()</script><p>a</p>`);
    expect(buildSandboxedPreviewDocument("")).toBe(POLICY);
  });
});

describe("AiChatFilePreview", () => {
  it("마크다운 파일은 렌더링해 보이고 원문으로 바꾸면 쓴 그대로 보인다", async () => {
    respondWith("# 계획\n\n- 첫째");
    render(<AiChatFilePreview taskId="task-1" filePath="/work/docs/plan.md" onClose={() => {}} />);

    expect(screen.getByRole("status").textContent).toBe("loading");
    expect((await screen.findByRole("heading", { name: "계획" })).tagName).toBe("H1");
    expect(readTaskWorkspaceFile).toHaveBeenCalledWith("task-1", "/work/docs/plan.md");

    fireEvent.click(screen.getByRole("button", { name: "source" }));
    expect(screen.getByTestId("ai-chat-file-source").textContent).toBe("# 계획\n\n- 첫째");
    expect(screen.queryByRole("heading", { name: "계획" })).toBeNull();
  });

  it("복사는 화면에 보이는 모양과 무관하게 파일 원문 전체를 복사한다", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    respondWith("# 계획\n\n- 첫째");
    render(<AiChatFilePreview taskId="task-1" filePath="docs/plan.md" onClose={() => {}} />);
    await screen.findByRole("heading", { name: "계획" });

    fireEvent.click(screen.getByRole("button", { name: "copy" }));

    expect(await screen.findByRole("button", { name: "copied" })).toBeTruthy();
    expect(writeText).toHaveBeenCalledWith("# 계획\n\n- 첫째");
  });

  it("HTML 파일은 스크립트만 허용하고 같은 출처와 바깥 요청을 막은 틀 안에 그린다", async () => {
    respondWith("<!doctype html><script>document.title = 1</script>");
    render(<AiChatFilePreview taskId="task-1" filePath="report.html" onClose={() => {}} />);

    const frame = await screen.findByTestId("ai-chat-file-document") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("srcdoc")).toBe(`<!doctype html>${POLICY}<script>document.title = 1</script>`);
    expect(frame.getAttribute("src")).toBeNull();
  });

  it("미리 볼 수 없는 종류의 파일은 보기 전환 없이 원문만 보인다", async () => {
    respondWith("const total = 3;");
    render(<AiChatFilePreview taskId="task-1" filePath="src/app.ts" onClose={() => {}} />);

    expect((await screen.findByTestId("ai-chat-file-source")).textContent).toBe("const total = 3;");
    expect(screen.queryByRole("button", { name: "rendered" })).toBeNull();
  });

  it("다른 파일로 바꾸면 이전 파일의 내용을 새 파일의 형태로 그리지 않고 불러오는 중으로 돌아간다", async () => {
    readTaskWorkspaceFile.mockResolvedValueOnce({ ok: true, file: { path: "src/app.ts", content: "const html = '<script>window.ran = 1</script>';" } });
    readTaskWorkspaceFile.mockReturnValue(new Promise(() => {}));
    const { rerender } = render(<AiChatFilePreview taskId="task-1" filePath="src/app.ts" onClose={() => {}} />);
    await screen.findByTestId("ai-chat-file-source");
    const mountedDocuments: Array<string | null> = [];
    const collectDocuments = (records: MutationRecord[]) => {
      for (const record of records) for (const node of record.addedNodes) {
        if (!(node instanceof Element)) continue;
        for (const frame of [node, ...node.querySelectorAll("iframe")]) {
          if (frame instanceof HTMLIFrameElement) mountedDocuments.push(frame.getAttribute("srcdoc"));
        }
      }
    };
    const observer = new MutationObserver(collectDocuments);
    observer.observe(document.body, { childList: true, subtree: true });

    rerender(<AiChatFilePreview taskId="task-1" filePath="report.html" onClose={() => {}} />);
    collectDocuments(observer.takeRecords());
    observer.disconnect();

    expect(mountedDocuments).toEqual([]);
    expect(screen.getByRole("status").textContent).toBe("loading");
    expect(screen.queryByTestId("ai-chat-file-document")).toBeNull();
    expect(readTaskWorkspaceFile).toHaveBeenLastCalledWith("task-1", "report.html");
  });

  it.each(["outside-worktree", "not-found", "too-large", "unreadable"] as const)("읽지 못한 사유 %s를 알리고 다시 시도하면 파일을 다시 읽는다", async (reason) => {
    readTaskWorkspaceFile.mockResolvedValueOnce({ ok: false, reason });
    respondWith("# 복구됨");
    render(<AiChatFilePreview taskId="task-1" filePath="docs/plan.md" onClose={() => {}} />);

    expect((await screen.findByRole("alert")).textContent).toContain(`failure.${reason}`);
    expect((screen.getByRole("button", { name: "copy" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "retry" }));
    expect(await screen.findByRole("heading", { name: "복구됨" })).toBeTruthy();
    expect(readTaskWorkspaceFile).toHaveBeenCalledTimes(2);
  });

  it("요청 자체가 실패해도 읽지 못했다고 알린다", async () => {
    readTaskWorkspaceFile.mockRejectedValue(new Error("ipc closed"));
    render(<AiChatFilePreview taskId="task-1" filePath="docs/plan.md" onClose={() => {}} />);

    expect((await screen.findByRole("alert")).textContent).toContain("failure.unreadable");
  });

  it("닫기를 누르면 닫힘을 알린다", async () => {
    respondWith("x");
    const onClose = vi.fn();
    render(<AiChatFilePreview taskId="task-1" filePath="notes.txt" onClose={onClose} />);
    await screen.findByTestId("ai-chat-file-source");

    fireEvent.click(screen.getByRole("button", { name: "close" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
