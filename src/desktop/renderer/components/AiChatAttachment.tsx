import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { getTaskAiSessionAttachment } from "@/desktop/renderer/actions/project";
import type { AiMessageAttachment, AiSessionAttachmentContent, AiSessionProvider } from "@/lib/aiSessions/types";

export interface AiChatAttachmentContext { taskId: string; provider: AiSessionProvider; sessionId: string; sourceRef?: string | null }

export function AiChatAttachment({ attachment, context }: { attachment: AiMessageAttachment; context?: AiChatAttachmentContext }) {
  const t = useTranslations("taskDetail.aiSessions");
  const [media, setMedia] = useState<AiSessionAttachmentContent | null>(null);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [visible, setVisible] = useState(typeof IntersectionObserver === "undefined");
  const figure = useRef<HTMLElement>(null);
  const generation = useRef(0);
  const taskId = context?.taskId;
  const provider = context?.provider;
  const sessionId = context?.sessionId;
  const sourceRef = context?.sourceRef;
  const invalidateRequests = useCallback(() => { ++generation.current; }, []);
  const load = useCallback(async () => {
    const epoch = generation.current;
    if (!taskId || !provider || !sessionId) { setFailed(true); return null; }
    setLoading(true);
    setFailed(false);
    try {
      const result = await getTaskAiSessionAttachment(taskId, provider, sessionId, attachment.id, sourceRef);
      if (!result || !/^data:[\w.+-]+\/[\w.+-]+;base64,[A-Za-z0-9+/]+=*$/.test(result.dataUrl) || (attachment.kind === "image" && !/^data:image\/(png|jpeg|gif|webp);base64,/.test(result.dataUrl))) throw new Error("Unavailable attachment");
      if (generation.current !== epoch) return null;
      setMedia(result);
      return result;
    } catch {
      if (generation.current === epoch) setFailed(true);
      return null;
    } finally {
      if (generation.current === epoch) setLoading(false);
    }
  }, [attachment.id, attachment.kind, taskId, provider, sessionId, sourceRef]);

  useEffect(() => {
    if (visible || !figure.current) return;
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) { setVisible(true); observer.disconnect(); }
    });
    observer.observe(figure.current);
    return () => observer.disconnect();
  }, [visible]);

  useEffect(() => {
    invalidateRequests();
    setMedia(null);
    setFailed(false);
    setLoading(false);
    setExpanded(false);
    if (attachment.kind === "image" && visible) void load();
    return invalidateRequests;
  }, [load, attachment.kind, visible, invalidateRequests]);

  async function download() {
    const result = media ?? await load();
    if (!result) return;
    const bytes = Uint8Array.from(atob(result.dataUrl.slice(result.dataUrl.indexOf(",") + 1)), character => character.charCodeAt(0));
    const url = URL.createObjectURL(new Blob([bytes], { type: result.mimeType }));
    const link = document.createElement("a");
    link.href = url;
    link.download = result.name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  return <figure ref={figure} className="my-2 min-w-0 max-w-full rounded-lg border border-border-subtle bg-bg-page p-2" aria-label={attachment.name}>
    {attachment.kind === "image" && media && !failed ? <button type="button" aria-expanded={expanded} aria-label={t(expanded ? "closeImage" : "openImage")} onClick={() => setExpanded(value => !value)} className="block rounded-md focus-visible:outline-2 focus-visible:outline-brand-primary">
      <img src={media.dataUrl} alt={attachment.name} loading="lazy" decoding="async" onError={() => setFailed(true)} className={`max-w-full rounded object-contain ${expanded ? "max-h-[70vh]" : "max-h-56"}`} />
    </button> : null}
    {loading ? <p role="status" className="px-2 py-4 text-xs text-text-secondary">{t("attachmentLoading")}</p> : null}
    {failed ? <div className="flex flex-wrap items-center gap-2 px-2 py-2 text-xs"><span role="status">{t("attachmentUnavailable")}</span><button type="button" onClick={() => void load()} className="min-h-9 rounded px-2 text-brand-primary focus-visible:outline-2 focus-visible:outline-brand-primary">{t("retry")}</button></div> : null}
    <figcaption className="flex min-w-0 items-center gap-2 px-1 text-xs text-text-secondary"><span className="min-w-0 flex-1 truncate" title={attachment.name}>{attachment.name}</span><button type="button" disabled={loading || !context} aria-label={`${t("downloadAttachment")}: ${attachment.name}`} onClick={() => void download()} className="min-h-9 shrink-0 rounded px-2 text-brand-primary focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40">{t("downloadAttachment")}</button></figcaption>
  </figure>;
}
