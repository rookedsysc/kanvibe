import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";
import { marked } from "marked";
import { useTranslations } from "next-intl";
import { type MouseEvent, useEffect, useId, useMemo, useState } from "react";

const MERMAID_BLOCK_PATTERN = /```[ \t]*mermaid[^\r\n]*(?:\r?\n)([\s\S]*?)```/gi;
const MARKDOWN_SANITIZE_CONFIG = {
  ADD_ATTR: ["target", "rel", "loading", "referrerpolicy"],
  FORBID_ATTR: ["style"],
  FORBID_TAGS: ["style"],
  USE_PROFILES: { html: true },
};
const MERMAID_SANITIZE_CONFIG = {
  ADD_ATTR: ["data-testid"],
};
const CODE_LANGUAGE_CLASS_PREFIX = "language-";
const COPY_FEEDBACK_DURATION_MS = 1_500;

interface AiSessionMessageContentProps {
  text: string;
}

interface MarkdownSegment {
  type: "markdown";
  value: string;
}

interface MermaidSegment {
  type: "mermaid";
  value: string;
}

type MessageSegment = MarkdownSegment | MermaidSegment;

function splitMarkdownAndMermaidBlocks(markdown: string): MessageSegment[] {
  const segments: MessageSegment[] = [];
  let cursor = 0;

  for (const match of markdown.matchAll(MERMAID_BLOCK_PATTERN)) {
    const blockStart = match.index ?? 0;
    const markdownValue = markdown.slice(cursor, blockStart);
    if (markdownValue.trim()) {
      segments.push({ type: "markdown", value: markdownValue });
    }

    const diagramValue = match[1]?.trim();
    if (diagramValue) {
      segments.push({ type: "mermaid", value: diagramValue });
    }

    cursor = blockStart + match[0].length;
  }

  const remainingMarkdown = markdown.slice(cursor);
  if (remainingMarkdown.trim()) {
    segments.push({ type: "markdown", value: remainingMarkdown });
  }

  return segments.length > 0 ? segments : [{ type: "markdown", value: markdown }];
}

/** 코드 블록마다 언어 이름과 복사 버튼이 놓인 머리줄을 붙이고, 아는 언어면 글자에 색을 입힌다 */
function decorateCodeBlocks(content: DocumentFragment, copyLabel: string) {
  for (const block of content.querySelectorAll("pre")) {
    const code = block.firstElementChild;
    if (code?.tagName !== "CODE") continue;

    const languageClass = [...code.classList].find((name) => name.startsWith(CODE_LANGUAGE_CLASS_PREFIX));
    const language = languageClass?.slice(CODE_LANGUAGE_CLASS_PREFIX.length) ?? "";
    if (language && hljs.getLanguage(language)) {
      code.innerHTML = hljs.highlight(code.textContent ?? "", { language }).value;
    }

    const languageLabel = document.createElement("span");
    languageLabel.textContent = language || "text";
    const copyButton = document.createElement("button");
    copyButton.type = "button";
    copyButton.dataset.copyCode = "";
    copyButton.textContent = copyLabel;
    const header = document.createElement("div");
    header.className = "kv-chat-code-header";
    header.append(languageLabel, copyButton);

    const frame = document.createElement("div");
    frame.className = "kv-chat-code";
    block.replaceWith(frame);
    frame.append(header, block);
  }
}

/** 열이 많은 표가 대화 폭을 밀어내지 않고 표 안에서만 옆으로 넘어가게 한다 */
function wrapTablesForHorizontalScroll(content: DocumentFragment) {
  for (const table of content.querySelectorAll("table")) {
    const frame = document.createElement("div");
    frame.className = "kv-chat-table";
    table.replaceWith(frame);
    frame.append(table);
  }
}

function decorateMarkdownElements(html: string, copyLabel: string) {
  const template = document.createElement("template");
  template.innerHTML = html;
  decorateCodeBlocks(template.content, copyLabel);
  wrapTablesForHorizontalScroll(template.content);

  for (const link of template.content.querySelectorAll("a")) {
    link.setAttribute("target", "_blank");
    link.setAttribute("rel", "noopener noreferrer");
  }

  for (const image of template.content.querySelectorAll("img")) {
    image.setAttribute("loading", "lazy");
    image.setAttribute("referrerpolicy", "no-referrer");
  }

  return template.innerHTML;
}

function renderMarkdownHtml(markdown: string, copyLabel: string) {
  const rawHtml = marked.parse(markdown, {
    async: false,
    breaks: true,
    gfm: true,
  }) as string;
  const sanitizedHtml = DOMPurify.sanitize(rawHtml, MARKDOWN_SANITIZE_CONFIG);
  return decorateMarkdownElements(sanitizedHtml, copyLabel);
}

function createMermaidElementId(definition: string, index: number, componentId: string) {
  let hash = 0;
  for (let charIndex = 0; charIndex < definition.length; charIndex += 1) {
    hash = (hash * 31 + definition.charCodeAt(charIndex)) | 0;
  }

  const stableComponentId = componentId.replace(/[^a-zA-Z0-9_-]/g, "");
  return `ai-session-mermaid-${stableComponentId}-${index}-${Math.abs(hash)}`;
}

function MarkdownContent({ markdown }: { markdown: string }) {
  const t = useTranslations("taskDetail");
  const copyLabel = t("aiSessions.copyMessage");
  const html = useMemo(() => renderMarkdownHtml(markdown, copyLabel), [markdown, copyLabel]);

  if (!html.trim()) {
    return null;
  }

  /** 본문은 HTML 문자열로 그려서 버튼마다 핸들러를 달 수 없다. 본문 전체에서 복사 버튼 클릭만 골라 받는다 */
  async function copyCodeBlock(event: MouseEvent<HTMLDivElement>) {
    const copyButton = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-copy-code]");
    if (!copyButton) return;
    const code = copyButton.closest(".kv-chat-code")?.querySelector("pre")?.textContent ?? "";
    try {
      await navigator.clipboard.writeText(code);
      copyButton.textContent = t("aiSessions.copiedMessage");
    } catch {
      copyButton.textContent = t("aiSessions.copyFailed");
    }
    window.setTimeout(() => {
      copyButton.textContent = copyLabel;
    }, COPY_FEEDBACK_DURATION_MS);
  }

  return (
    <div
      className="ai-session-markdown text-sm leading-6 [&_*:first-child]:mt-0 [&_*:last-child]:mb-0 [&_a]:text-brand-primary [&_a]:underline-offset-2 [&_a:hover]:underline [&_blockquote]:my-3 [&_blockquote]:border-l-2 [&_blockquote]:border-border-default [&_blockquote]:pl-3 [&_blockquote]:text-text-muted [&_code]:rounded [&_code]:bg-bg-page [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.86em] [&_h1]:mb-3 [&_h1]:mt-5 [&_h1]:text-lg [&_h1]:font-semibold [&_h1]:text-text-primary [&_h2]:mb-2 [&_h2]:mt-4 [&_h2]:text-base [&_h2]:font-semibold [&_h2]:text-text-primary [&_h3]:mb-2 [&_h3]:mt-4 [&_h3]:text-sm [&_h3]:font-semibold [&_h3]:text-text-primary [&_hr]:my-4 [&_hr]:border-border-subtle [&_li]:my-1 [&_ol]:my-3 [&_ol]:list-decimal [&_ol]:pl-5 [&_p]:my-3 [&_pre]:overflow-x-auto [&_pre]:p-3 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_strong]:font-semibold [&_strong]:text-text-primary [&_table]:w-max [&_table]:min-w-full [&_table]:border-collapse [&_td]:max-w-xs [&_td]:border [&_td]:border-border-default [&_td]:px-3 [&_td]:py-1.5 [&_td]:align-top [&_th]:max-w-xs [&_th]:border [&_th]:border-border-default [&_th]:bg-bg-page [&_th]:px-3 [&_th]:py-1.5 [&_th]:text-left [&_th]:align-top [&_ul]:my-3 [&_ul]:list-disc [&_ul]:pl-5"
      onClick={(event) => void copyCodeBlock(event)}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function MermaidDiagram({ definition, index }: { definition: string; index: number }) {
  const [svg, setSvg] = useState<string | null>(null);
  const [hasError, setHasError] = useState(false);
  const componentId = useId();
  const elementId = useMemo(() => createMermaidElementId(definition, index, componentId), [definition, index, componentId]);

  useEffect(() => {
    let cancelled = false;
    setSvg(null);
    setHasError(false);

    async function renderDiagram() {
      try {
        const mermaidModule = await import("mermaid");
        const mermaidApi = mermaidModule.default;
        mermaidApi.initialize({
          securityLevel: "strict",
          startOnLoad: false,
          // SVG text survives sanitization, while HTML foreignObject labels are removed.
          htmlLabels: false,
          theme: document.documentElement.dataset.theme === "light" ? "default" : "dark",
        });
        const result = await mermaidApi.render(elementId, definition);
        const sanitizedSvg = DOMPurify.sanitize(result.svg, MERMAID_SANITIZE_CONFIG);
        if (!cancelled) {
          setSvg(sanitizedSvg);
        }
      } catch {
        if (!cancelled) {
          setHasError(true);
        }
      }
    }

    void renderDiagram();

    return () => {
      cancelled = true;
    };
  }, [definition, elementId]);

  if (hasError) {
    return (
      <pre
        data-testid="ai-session-mermaid-fallback"
        className="my-3 overflow-x-auto rounded-lg border border-border-subtle bg-bg-page p-3 font-mono text-xs text-text-primary"
      >
        <code>{definition}</code>
      </pre>
    );
  }

  return (
    <div
      data-testid="ai-session-mermaid-diagram"
      className="my-3 overflow-x-auto rounded-lg border border-border-subtle bg-bg-page p-3"
    >
      {svg ? (
        <div
          className="min-w-max [&_svg]:h-auto [&_svg]:max-w-full"
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      ) : (
        <p className="text-xs text-text-muted">Rendering diagram…</p>
      )}
    </div>
  );
}

export function AiSessionMessageContent({ text }: AiSessionMessageContentProps) {
  const segments = useMemo(() => splitMarkdownAndMermaidBlocks(text), [text]);

  return (
    <div className="space-y-3 break-words">
      {segments.map((segment, index) => (
        segment.type === "mermaid" ? (
          <MermaidDiagram
            key={`mermaid-${index}`}
            definition={segment.value}
            index={index}
          />
        ) : (
          <MarkdownContent
            key={`markdown-${index}`}
            markdown={segment.value}
          />
        )
      ))}
    </div>
  );
}
