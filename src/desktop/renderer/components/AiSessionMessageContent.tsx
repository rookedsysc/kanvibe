import DOMPurify from "dompurify";
import { marked } from "marked";
import { useEffect, useId, useMemo, useState } from "react";

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
const MERMAID_THEME = "dark";

interface AiSessionMessageContentProps {
  text: string;
  isUserMessage: boolean;
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

function addMarkdownElementAttributes(html: string) {
  const template = document.createElement("template");
  template.innerHTML = html;

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

function renderMarkdownHtml(markdown: string) {
  const rawHtml = marked.parse(markdown, {
    async: false,
    breaks: true,
    gfm: true,
  }) as string;
  const sanitizedHtml = DOMPurify.sanitize(rawHtml, MARKDOWN_SANITIZE_CONFIG);
  return addMarkdownElementAttributes(sanitizedHtml);
}

function createMermaidElementId(definition: string, index: number, componentId: string) {
  let hash = 0;
  for (let charIndex = 0; charIndex < definition.length; charIndex += 1) {
    hash = (hash * 31 + definition.charCodeAt(charIndex)) | 0;
  }

  const stableComponentId = componentId.replace(/[^a-zA-Z0-9_-]/g, "");
  return `ai-session-mermaid-${stableComponentId}-${index}-${Math.abs(hash)}`;
}

function MarkdownContent({ markdown, isUserMessage }: { markdown: string; isUserMessage: boolean }) {
  const html = useMemo(() => renderMarkdownHtml(markdown), [markdown]);

  if (!html.trim()) {
    return null;
  }

  return (
    <div
      className={`ai-session-markdown text-sm leading-6 [&_*:first-child]:mt-0 [&_*:last-child]:mb-0 [&_a]:underline-offset-2 [&_a:hover]:underline [&_blockquote]:my-3 [&_blockquote]:border-l-2 [&_blockquote]:pl-3 [&_code]:rounded [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.86em] [&_h1]:mb-3 [&_h1]:mt-5 [&_h1]:text-lg [&_h1]:font-semibold [&_h2]:mb-2 [&_h2]:mt-4 [&_h2]:text-base [&_h2]:font-semibold [&_h3]:mb-2 [&_h3]:mt-4 [&_h3]:text-sm [&_h3]:font-semibold [&_hr]:my-4 [&_li]:my-1 [&_ol]:my-3 [&_ol]:list-decimal [&_ol]:pl-5 [&_p]:my-3 [&_pre]:my-3 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:border [&_pre]:p-3 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_strong]:font-semibold [&_table]:my-3 [&_table]:w-full [&_table]:border-separate [&_table]:border-spacing-2 [&_td]:align-top [&_th]:align-top [&_ul]:my-3 [&_ul]:list-disc [&_ul]:pl-5 ${
        isUserMessage
          ? "[&_a]:text-white [&_blockquote]:border-white/35 [&_blockquote]:text-white/75 [&_code]:bg-white/15 [&_h1]:text-white [&_h2]:text-white [&_h3]:text-white [&_hr]:border-white/20 [&_pre]:border-white/20 [&_pre]:bg-black/20 [&_strong]:text-white"
          : "[&_a]:text-brand-primary [&_blockquote]:border-border-default [&_blockquote]:text-text-muted [&_code]:bg-bg-page [&_h1]:text-text-primary [&_h2]:text-text-primary [&_h3]:text-text-primary [&_hr]:border-border-subtle [&_pre]:border-border-subtle [&_pre]:bg-bg-page [&_strong]:text-text-primary"
      }`}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function MermaidDiagram({ definition, index, isUserMessage }: { definition: string; index: number; isUserMessage: boolean }) {
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
          theme: MERMAID_THEME,
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
        className={`my-3 overflow-x-auto rounded-lg border p-3 font-mono text-xs ${
          isUserMessage ? "border-white/20 bg-black/20 text-white" : "border-border-subtle bg-bg-page text-text-primary"
        }`}
      >
        <code>{definition}</code>
      </pre>
    );
  }

  return (
    <div
      data-testid="ai-session-mermaid-diagram"
      className={`my-3 overflow-x-auto rounded-lg border p-3 ${
        isUserMessage ? "border-white/20 bg-white/10" : "border-border-subtle bg-bg-page"
      }`}
    >
      {svg ? (
        <div
          className="min-w-max [&_svg]:h-auto [&_svg]:max-w-full"
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      ) : (
        <p className={isUserMessage ? "text-xs text-white/70" : "text-xs text-text-muted"}>Rendering diagram…</p>
      )}
    </div>
  );
}

export function AiSessionMessageContent({ text, isUserMessage }: AiSessionMessageContentProps) {
  const segments = useMemo(() => splitMarkdownAndMermaidBlocks(text), [text]);

  return (
    <div className="space-y-3 break-words">
      {segments.map((segment, index) => (
        segment.type === "mermaid" ? (
          <MermaidDiagram
            key={`mermaid-${index}`}
            definition={segment.value}
            index={index}
            isUserMessage={isUserMessage}
          />
        ) : (
          <MarkdownContent
            key={`markdown-${index}`}
            markdown={segment.value}
            isUserMessage={isUserMessage}
          />
        )
      ))}
    </div>
  );
}
