import { useState } from "react";
import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { AI_SESSION_PROVIDER_LABELS } from "@/lib/aiSessions/providers";
import type { AggregatedAiMessage, AiSessionProvider } from "@/lib/aiSessions/types";
import { AiSessionMessageContent } from "./AiSessionMessageContent";
import { AiChatAttachment, type AiChatAttachmentContext } from "./AiChatAttachment";

export function AiChatTranscriptMessage({ message, provider, attachmentContext }: { message: AggregatedAiMessage; provider: AiSessionProvider; attachmentContext?: AiChatAttachmentContext }) {
  const t = useTranslations("taskDetail");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const text = message.fullText || message.text;
  const isUser = message.role === "user";
  const isDisclosure = message.role === "tool" || message.role === "reasoning";
  const timestamp = message.timestamp ? new Date(message.timestamp) : null;
  const copyLabel = t(`aiSessions.${copyState === "idle" ? "copyMessage" : copyState === "copied" ? "copiedMessage" : "copyFailed"}`);

  async function copyMessage() {
    try {
      await navigator.clipboard.writeText(text);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  }

  const meta = (
    <div className="mt-1 flex min-h-7 w-fit max-w-full flex-wrap items-center gap-x-2 text-xs text-text-secondary">
      {!isUser && <AiProviderIcon provider={provider} size={16} />}
      {!isUser && <span className="font-medium">{message.role === "assistant" ? AI_SESSION_PROVIDER_LABELS[provider] : t(`aiSessions.roles.${message.role}`)}</span>}
      {timestamp && Number.isFinite(timestamp.getTime()) ? (
        <time dateTime={message.timestamp!} title={timestamp.toLocaleString()} className="text-text-secondary tabular-nums">
          {timestamp.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </time>
      ) : null}
      {text ? <button
        type="button"
        onClick={() => void copyMessage()}
        className="min-h-7 rounded px-2 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-surface hover:text-text-primary focus-visible:outline-2 focus-visible:outline-brand-primary"
        aria-label={copyLabel}
      >
        <span aria-live="polite">{copyLabel}</span>
      </button> : null}
    </div>
  );
  const body = <>{text ? <AiSessionMessageContent text={text} isUserMessage={false} /> : null}{message.attachments?.map((attachment, index) => <AiChatAttachment key={`${attachment.id}:${index}`} attachment={attachment} context={attachmentContext} />)}</>;

  if (isDisclosure) {
    return (
      <details className="group rounded-lg border border-border-subtle bg-bg-surface px-4 py-2" data-message-role={message.role}>
        <summary className="cursor-pointer py-1 text-xs font-medium text-text-secondary focus-visible:outline-2 focus-visible:outline-brand-primary">
          {t(`aiSessions.roles.${message.role}`)}
          <span className="ml-2 inline-block max-w-[65%] truncate align-bottom font-normal text-text-secondary">{text.split("\n")[0]}</span>
        </summary>
        <div className="mt-2 min-w-0 pb-2 text-sm leading-6 text-text-primary">{body}{meta}</div>
      </details>
    );
  }

  return (
    <article className={`flex min-w-0 ${isUser ? "justify-end" : "justify-start"}`} data-message-role={message.role} aria-label={isUser ? t("aiSessions.roles.user") : undefined}>
      <div className={isUser
        ? "min-w-0 max-w-[88%] rounded-2xl rounded-br-md bg-bg-surface px-4 py-2 text-sm leading-6 text-text-primary"
        : "min-w-0 w-full py-1 text-sm leading-7 text-text-primary"}>
        {body}
        {meta}
      </div>
    </article>
  );
}
