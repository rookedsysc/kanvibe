import hljs from "highlight.js/lib/common";
import { useEffect, useMemo, useState } from "react";
import { useTranslations } from "next-intl";
import { readTaskWorkspaceFile } from "@/desktop/renderer/actions/diff";
import { AiSessionMessageContent } from "./AiSessionMessageContent";

type PreviewKind = "markdown" | "document" | "source";
type PreviewMode = "rendered" | "source";
type FailureReason = "outside-worktree" | "not-found" | "too-large" | "unreadable";

const MARKDOWN_EXTENSIONS = new Set(["md", "markdown", "mdx"]);
const DOCUMENT_EXTENSIONS = new Set(["html", "htm", "svg"]);
const FAILURE_REASONS: readonly FailureReason[] = ["outside-worktree", "not-found", "too-large", "unreadable"];
const COPY_FEEDBACK_DURATION_MS = 1_500;

/**
 * AI가 만든 문서 안의 스크립트는 그 문서를 그리는 데만 쓴다.
 * 바깥으로 나가는 요청을 모두 막아, 파일 내용이 다른 곳으로 보내지거나 외부 코드가 끌려오지 않게 한다.
 */
const PREVIEW_CONTENT_SECURITY_POLICY =
  "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; form-action 'none'";

function readFileExtension(filePath: string) {
  const fileName = filePath.split("/").at(-1) ?? "";
  return fileName.includes(".") ? fileName.split(".").at(-1)!.toLowerCase() : "";
}

function resolvePreviewKind(filePath: string): PreviewKind {
  const extension = readFileExtension(filePath);
  if (MARKDOWN_EXTENSIONS.has(extension)) return "markdown";
  if (DOCUMENT_EXTENSIONS.has(extension)) return "document";
  return "source";
}

/**
 * 문서가 스스로 실행을 시작하기 전에 정책이 먼저 읽히도록 맨 앞에 넣는다.
 * doctype 앞에 다른 것이 오면 브라우저가 문서를 호환 모드로 그리므로, doctype이 있으면 그 바로 뒤에 넣는다.
 */
export function buildSandboxedPreviewDocument(content: string) {
  const policy = `<meta http-equiv="Content-Security-Policy" content="${PREVIEW_CONTENT_SECURITY_POLICY}">`;
  const doctype = /^\s*<!doctype[^>]*>/i.exec(content)?.[0] ?? "";
  return `${doctype}${policy}${content.slice(doctype.length)}`;
}

function SourceView({ filePath, content }: { filePath: string; content: string }) {
  const extension = readFileExtension(filePath);
  const highlighted = useMemo(
    () => (extension && hljs.getLanguage(extension) ? hljs.highlight(content, { language: extension }).value : null),
    [content, extension],
  );

  return (
    <pre data-testid="ai-chat-file-source" className="kv-chat-code m-0! min-h-full overflow-auto! rounded-none! border-0! p-4 font-mono text-xs leading-5 text-text-primary">
      {highlighted === null ? <code>{content}</code> : <code dangerouslySetInnerHTML={{ __html: highlighted }} />}
    </pre>
  );
}

type AiChatFilePreviewProps = { taskId: string; filePath: string; onClose: () => void };

/**
 * 파일마다 패널을 새로 만든다.
 * 같은 패널에 경로만 바꿔 주면 새 파일을 읽기 전의 첫 화면에 이전 파일의 내용이 새 파일의 형태로 그려진다.
 */
export function AiChatFilePreview(props: AiChatFilePreviewProps) {
  return <FilePreviewPanel key={`${props.taskId}\n${props.filePath}`} {...props} />;
}

function FilePreviewPanel({ taskId, filePath, onClose }: AiChatFilePreviewProps) {
  const t = useTranslations("taskDetail.aiSessions.filePreview");
  const [content, setContent] = useState<string | null>(null);
  const [failure, setFailure] = useState<FailureReason | null>(null);
  const [mode, setMode] = useState<PreviewMode>("rendered");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const [loadAttempt, setLoadAttempt] = useState(0);
  const kind = resolvePreviewKind(filePath);
  const fileName = filePath.split("/").at(-1) ?? filePath;

  useEffect(() => {
    let cancelled = false;
    setContent(null);
    setFailure(null);
    setMode("rendered");

    readTaskWorkspaceFile(taskId, filePath)
      .then((result) => {
        if (cancelled) return;
        if (result.ok) setContent(result.file.content);
        else setFailure(FAILURE_REASONS.includes(result.reason) ? result.reason : "unreadable");
      })
      .catch(() => {
        if (!cancelled) setFailure("unreadable");
      });

    return () => {
      cancelled = true;
    };
  }, [taskId, filePath, loadAttempt]);

  useEffect(() => {
    if (copyState === "idle") return;
    const timer = window.setTimeout(() => setCopyState("idle"), COPY_FEEDBACK_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [copyState]);

  async function copyContent() {
    if (content === null) return;
    try {
      await navigator.clipboard.writeText(content);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  }

  const buttonClass = "min-h-8 rounded-md px-2 text-xs text-text-secondary hover:bg-bg-page hover:text-text-primary focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40";
  const showsRendered = kind !== "source" && mode === "rendered";

  return (
    <aside data-testid="ai-chat-file-preview" aria-label={t("title", { name: fileName })} className="absolute inset-y-0 right-0 z-10 flex w-full max-w-2xl flex-col border-l border-border-default bg-bg-surface shadow-lg">
      <header className="flex min-h-12 items-center gap-2 border-b border-border-subtle px-3">
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-sm font-medium text-text-primary">{fileName}</h3>
          <p title={filePath} className="truncate text-xs text-text-secondary">{filePath}</p>
        </div>
        {kind !== "source" ? (
          <div role="group" aria-label={t("viewMode")} className="flex shrink-0 rounded-md border border-border-default">
            {(["rendered", "source"] as const).map((option) => (
              <button key={option} type="button" aria-pressed={mode === option} onClick={() => setMode(option)} className={`${buttonClass} ${mode === option ? "bg-bg-page text-text-primary" : ""}`}>
                {t(option)}
              </button>
            ))}
          </div>
        ) : null}
        <button type="button" disabled={content === null} onClick={() => void copyContent()} className={buttonClass}>
          <span aria-live="polite">{t(copyState === "idle" ? "copy" : copyState === "copied" ? "copied" : "copyFailed")}</span>
        </button>
        <button type="button" onClick={onClose} aria-label={t("close")} className={buttonClass}>✕</button>
      </header>
      <div className="min-h-0 flex-1 overflow-auto">
        {content === null && failure === null ? <p role="status" className="p-4 text-sm text-text-secondary">{t("loading")}</p> : null}
        {failure ? (
          <div role="alert" className="space-y-3 p-4 text-sm text-status-error">
            <p>{t(`failure.${failure}`)}</p>
            <button type="button" onClick={() => setLoadAttempt((attempt) => attempt + 1)} className={buttonClass}>{t("retry")}</button>
          </div>
        ) : null}
        {content !== null && showsRendered && kind === "markdown" ? <div className="p-4"><AiSessionMessageContent text={content} /></div> : null}
        {content !== null && showsRendered && kind === "document" ? (
          <iframe
            data-testid="ai-chat-file-document"
            title={fileName}
            sandbox="allow-scripts"
            referrerPolicy="no-referrer"
            srcDoc={buildSandboxedPreviewDocument(content)}
            className="h-full w-full border-0 bg-white"
          />
        ) : null}
        {content !== null && !showsRendered ? <SourceView filePath={filePath} content={content} /> : null}
      </div>
    </aside>
  );
}
