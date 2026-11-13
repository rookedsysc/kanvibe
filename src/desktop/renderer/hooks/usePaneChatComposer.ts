import { useEffect, useRef, useState } from "react";
import * as actions from "@/desktop/renderer/actions/paneChat";
import type { PaneChatAttachment, PaneChatSnapshot } from "@/desktop/shared/paneChat";

interface PendingAttachment {
  localId: string;
  file: File;
  preview?: string;
  staged?: PaneChatAttachment;
  status: "uploading" | "ready" | "failed" | "removing";
  error?: string;
}
interface Draft {
  text: string;
  revision: number;
  attachments: PendingAttachment[];
  sending: boolean;
  error: string | null;
  request?: { fingerprint: string; id: string };
}
const emptyDraft = (): Draft => ({ text: "", revision: 0, attachments: [], sending: false, error: null });

function readAttachment(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === "string" ? resolve(reader.result) : reject(new Error("Could not read file"));
    reader.onerror = () => reject(new Error("Could not read file"));
    reader.readAsDataURL(file);
  });
}

export function usePaneChatComposer(taskId: string, snapshot: PaneChatSnapshot | null, send: (text: string, ids?: string[], requestId?: string) => Promise<boolean>) {
  const key = `${taskId}:${snapshot?.targetToken ?? "unavailable"}`;
  const drafts = useRef<Record<string, Draft>>({});
  const [, render] = useState(0);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  function read(target = key) { return drafts.current[target] ?? emptyDraft(); }
  function update(target: string, mutate: (draft: Draft) => Draft) {
    drafts.current[target] = mutate(read(target));
    if (mounted.current) render((value) => value + 1);
  }
  const draft = read();

  async function upload(target: string, token: string, item: PendingAttachment) {
    update(target, (value) => ({ ...value, attachments: value.attachments.map((entry) => entry.localId === item.localId ? { ...entry, status: "uploading", error: undefined } : entry) }));
    try {
      const dataUrl = await readAttachment(item.file);
      update(target, (value) => ({ ...value, attachments: value.attachments.map((entry) => entry.localId === item.localId ? { ...entry, preview: item.file.type.startsWith("image/") ? dataUrl : undefined } : entry) }));
      if (!mounted.current || read(target).attachments.find((entry) => entry.localId === item.localId)?.status === "removing") {
        update(target, (value) => ({ ...value, attachments: value.attachments.filter((entry) => entry.localId !== item.localId) }));
        return;
      }
      const staged = await actions.stagePaneChatAttachment(taskId, token, { name: item.file.name, mimeType: item.file.type || "application/octet-stream", dataUrl });
      if (!mounted.current) {
        try { await actions.removePaneChatAttachment(taskId, token, staged.id); }
        catch (error) { console.warn("Could not remove an abandoned chat attachment", error); }
        return;
      }
      const removing = read(target).attachments.find((entry) => entry.localId === item.localId)?.status === "removing";
      update(target, (value) => ({ ...value, attachments: value.attachments.map((entry) => entry.localId === item.localId ? { ...entry, staged, status: removing ? "removing" : "ready", error: undefined } : entry) }));
      if (removing) {
        await actions.removePaneChatAttachment(taskId, token, staged.id);
        update(target, (value) => ({ ...value, attachments: value.attachments.filter((entry) => entry.localId !== item.localId) }));
      }
    } catch (error) {
      update(target, (value) => ({ ...value, attachments: value.attachments.map((entry) => entry.localId === item.localId ? { ...entry, status: entry.staged ? "ready" : "failed", error: error instanceof Error ? error.message : "Upload failed" } : entry) }));
    }
  }

  function addFiles(files: File[]) {
    const token = snapshot?.targetToken;
    if (!token || !snapshot?.composer?.attachments) return;
    const limit = snapshot.composer.maxAttachments;
    const remaining = limit - read().attachments.length;
    if (files.length > remaining) {
      update(key, (value) => ({ ...value, error: "attachmentLimit" }));
      return;
    }
    const items = files.map((file): PendingAttachment => ({ localId: crypto.randomUUID(), file, status: file.size > snapshot.composer!.maxAttachmentBytes ? "failed" : "uploading", error: file.size > snapshot.composer!.maxAttachmentBytes ? "attachmentTooLarge" : undefined }));
    update(key, (value) => ({ ...value, error: null, attachments: [...value.attachments, ...items] }));
    for (const item of items) if (item.status === "uploading") void upload(key, token, item);
  }

  async function remove(localId: string) {
    const token = snapshot?.targetToken;
    const item = read().attachments.find((entry) => entry.localId === localId);
    if (!item || item.status === "removing" || read().sending) return;
    if (item.status === "uploading") {
      update(key, (value) => ({ ...value, attachments: value.attachments.map((entry) => entry.localId === localId ? { ...entry, status: "removing" } : entry) }));
      return;
    }
    if (item.staged && token) {
      update(key, (value) => ({ ...value, attachments: value.attachments.map((entry) => entry.localId === localId ? { ...entry, status: "removing", error: undefined } : entry) }));
      try { await actions.removePaneChatAttachment(taskId, token, item.staged.id); }
      catch (error) {
        update(key, (value) => ({ ...value, attachments: value.attachments.map((entry) => entry.localId === localId ? { ...entry, status: "ready", error: error instanceof Error ? error.message : "Remove failed" } : entry) }));
        return;
      }
    }
    update(key, (value) => ({ ...value, attachments: value.attachments.filter((entry) => entry.localId !== localId) }));
  }

  function retry(localId: string) {
    const item = read().attachments.find((entry) => entry.localId === localId);
    if (item?.status === "failed" && snapshot?.targetToken && item.file.size <= (snapshot.composer?.maxAttachmentBytes ?? 0)) void upload(key, snapshot.targetToken, item);
  }

  async function submit() {
    const captured = read();
    if (captured.sending || (!captured.text.trim() && !captured.attachments.length) || captured.attachments.some((entry) => entry.status !== "ready" || entry.error)) return;
    const ids = captured.attachments.map((entry) => entry.staged!.id);
    const fingerprint = JSON.stringify([captured.text, ids]);
    const request = captured.request?.fingerprint === fingerprint ? captured.request : { fingerprint, id: crypto.randomUUID() };
    update(key, (value) => ({ ...value, sending: true, error: null, request }));
    const accepted = await send(captured.text, ids, request.id);
    update(key, (value) => ({ ...value, sending: false, error: accepted ? null : "sendError", ...(accepted ? {
      text: value.revision === captured.revision ? "" : value.text,
      attachments: value.attachments.filter((entry) => !captured.attachments.some((sent) => sent.localId === entry.localId)),
      request: undefined,
    } : {}) }));
  }

  return { ...draft, addFiles, remove, retry, submit, setText: (text: string) => update(key, (value) => ({ ...value, text, revision: value.revision + 1, error: null })) };
}
