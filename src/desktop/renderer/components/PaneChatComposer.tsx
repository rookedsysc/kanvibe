import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslations } from "next-intl";
import { getPaneChatSuggestions } from "@/desktop/renderer/actions/paneChat";
import { usePaneChatComposer } from "@/desktop/renderer/hooks/usePaneChatComposer";
import type { usePaneChat } from "@/desktop/renderer/hooks/usePaneChat";
import type { PaneChatSuggestion, PaneChatSuggestionKind } from "@/desktop/shared/paneChat";

const SUGGESTION_DEBOUNCE_MS = 180;
const controlClass = "min-h-10 shrink-0 rounded-md px-2 text-xs text-text-secondary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40";
interface Completion { kind: PaneChatSuggestionKind; query: string; start: number; end: number }

export function PaneChatComposer({ taskId, chat, controlsOpen, isVisible, onModel, onControls }: {
  taskId: string;
  chat: ReturnType<typeof usePaneChat>;
  controlsOpen: boolean;
  isVisible: boolean;
  onModel: () => void;
  onControls: () => void;
}) {
  const t = useTranslations("taskDetail.paneChat");
  const composer = usePaneChatComposer(taskId, chat.snapshot, chat.send);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const photoInput = useRef<HTMLInputElement>(null);
  const skillSearch = useRef<HTMLInputElement>(null);
  const [skillQuery, setSkillQuery] = useState("");
  const [caret, setCaret] = useState(0);
  const [skillPicker, setSkillPicker] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [suggestions, setSuggestions] = useState<{ key: string; items: PaneChatSuggestion[]; loading: boolean; error: boolean; truncated: boolean } | null>(null);
  const [active, setActive] = useState(0);
  const [retry, setRetry] = useState(0);
  const listId = useId();
  const target = chat.snapshot?.targetToken;
  const capabilities = chat.snapshot?.composer;
  const inputReady = chat.snapshot?.inputState === "ready";
  const disabled = !chat.isReady || !chat.snapshot?.canSend || chat.readError || chat.isBusy;
  const canEdit = chat.isReady && !!target;
  const token = /(?:^|\s)([$/@])([^\s]*)$/.exec(composer.text.slice(0, caret));
  const completion: Completion | null = skillPicker ? { kind: "skill", query: skillQuery, start: caret, end: caret } : token ? { kind: token[1] === "$" ? "skill" : token[1] === "/" ? "command" : "file", query: token[2], start: caret - token[2].length - 1, end: caret } : null;
  const supported = completion?.kind === "file" ? capabilities?.fileMentions : capabilities?.skills;
  const completionKey = completion && supported && isVisible && canEdit && !controlsOpen ? JSON.stringify([taskId, target, composer.text, caret, skillPicker, completion.kind, completion.query]) : null;
  const showSuggestions = !!completionKey && completionKey !== dismissed;
  const currentSuggestions = suggestions?.key === completionKey ? suggestions : null;

  useEffect(() => {
    setSkillPicker(false);
    setDismissed(null);
    setCaret(0);
  }, [target, taskId]);

  useEffect(() => { if (skillPicker) skillSearch.current?.focus(); }, [skillPicker]);

  useEffect(() => {
    const pane = textarea.current?.closest("[data-pane-chat-id]");
    if (isVisible && canEdit && !controlsOpen && pane?.contains(document.activeElement)) textarea.current?.focus();
  }, [isVisible, canEdit, controlsOpen, target]);

  const kind = completion?.kind;
  const query = completion?.query;
  useEffect(() => {
    if (!showSuggestions || !completionKey || !target || !kind || query === undefined) return;
    let disposed = false;
    setActive(0);
    setSuggestions({ key: completionKey, items: [], loading: true, error: false, truncated: false });
    const timer = setTimeout(() => {
      void getPaneChatSuggestions(taskId, target, kind, query).then((result) => {
      if (!disposed) setSuggestions({ key: completionKey, items: result.targetToken === target ? result.items : [], loading: false, error: result.targetToken !== target, truncated: result.truncated });
    }).catch(() => {
      if (!disposed) setSuggestions({ key: completionKey, items: [], loading: false, error: true, truncated: false });
    });
    }, SUGGESTION_DEBOUNCE_MS);
    return () => { disposed = true; clearTimeout(timer); };
  }, [showSuggestions, completionKey, taskId, target, kind, query, retry]);

  useEffect(() => {
    if (showSuggestions) document.getElementById(`${listId}-${active}`)?.scrollIntoView?.({ block: "nearest" });
  }, [showSuggestions, listId, active, currentSuggestions]);

  function choose(item: PaneChatSuggestion) {
    if (!completion) return;
    const prefix = composer.text.slice(0, completion.start);
    const suffix = composer.text.slice(completion.end);
    const insertion = `${prefix && !/\s$/.test(prefix) ? " " : ""}${item.insertText} `;
    const text = prefix + insertion + suffix;
    const position = prefix.length + insertion.length;
    composer.setText(text);
    setSkillPicker(false);
    setCaret(position);
    setDismissed(JSON.stringify([taskId, target, text, position, false, completion.kind, ""]));
    requestAnimationFrame(() => { textarea.current?.focus(); textarea.current?.setSelectionRange(position, position); });
  }
  function submit() {
    if (disabled || controlsOpen || !inputReady) return;
    setSkillPicker(false);
    setDismissed(completionKey);
    void composer.submit();
  }
  function handleCompletionKey(event: KeyboardEvent<HTMLTextAreaElement | HTMLInputElement>) {
    if (event.nativeEvent.isComposing) return false;
    if (!showSuggestions) return false;
    const items = currentSuggestions?.items ?? [];
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && items.length) {
      event.preventDefault();
      setActive((value) => (value + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length);
      return true;
    }
    if (event.key === "Escape") { event.preventDefault(); setDismissed(completionKey); setSkillPicker(false); textarea.current?.focus(); return true; }
    if ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab") {
      if (items[active]) { event.preventDefault(); choose(items[active]); return true; }
      if (event.key === "Enter") { event.preventDefault(); return true; }
    }
    return false;
  }
  const hasPending = composer.attachments.some((item) => item.status !== "ready" || item.error);
  return (
    <form onSubmit={(event) => { event.preventDefault(); submit(); }} className="shrink-0 overflow-y-auto [scrollbar-gutter:stable] px-4 py-3 sm:px-8">
      <div data-testid="pane-chat-composer" className="mx-auto max-w-3xl rounded-xl border border-border-default bg-bg-surface p-2 focus-within:border-brand-primary" onDragOver={(event) => {
        if (capabilities?.attachments && canEdit && event.dataTransfer.types.includes("Files")) event.preventDefault();
      }} onDrop={(event) => {
        if (!capabilities?.attachments || !canEdit || !event.dataTransfer.files.length) return;
        event.preventDefault();
        composer.addFiles(Array.from(event.dataTransfer.files));
      }}>
        {composer.attachments.length ? <ul aria-label={t("attachments")} className="flex flex-wrap gap-2 px-2 py-1">
          {composer.attachments.map((item) => <li key={item.localId} className="flex max-w-full items-center gap-2 rounded-lg bg-bg-page px-2 py-1">
            {item.preview ? <img src={item.preview} alt={item.file.name} className="h-14 w-14 rounded object-cover" /> : <span aria-hidden="true" className="text-xs text-text-secondary">↗</span>}
            <div className="min-w-0"><p className="max-w-44 truncate text-xs text-text-primary" title={item.file.name}>{item.file.name}</p>
              <p className="text-xs text-text-secondary">{Math.ceil(item.file.size / 1024)} KB</p>
              {item.status === "uploading" || item.status === "removing" ? <p role="status" className="text-xs text-text-secondary">{t(item.status === "uploading" ? "uploading" : "removingAttachment")}</p> : null}
              {item.error ? <p role="alert" className="max-w-56 text-xs text-status-error">{item.error === "attachmentTooLarge" ? t("attachmentTooLarge") : item.error}</p> : null}
            </div>
            {item.status === "failed" && item.error !== "attachmentTooLarge" ? <button type="button" aria-label={`${t("retryAttachment")} ${item.file.name}`} disabled={!canEdit} onClick={() => composer.retry(item.localId)} className={controlClass}>{t("retryAttachment")}</button> : null}
            <button type="button" aria-label={`${t("removeAttachment")} ${item.file.name}`} disabled={!canEdit || item.status === "removing" || composer.sending} onClick={() => void composer.remove(item.localId)} className={controlClass}>×</button>
          </li>)}
        </ul> : null}
        {showSuggestions ? <div className="mb-2 rounded-lg border border-border-subtle bg-bg-page p-1">
          {skillPicker ? <input ref={skillSearch} aria-label={t("searchSkills")} aria-autocomplete="list" aria-controls={listId} aria-activedescendant={currentSuggestions?.items[active] ? `${listId}-${active}` : undefined} placeholder={t("searchSkills")} value={skillQuery} onChange={(event) => setSkillQuery(event.target.value)} onKeyDown={handleCompletionKey} className="mb-1 min-h-11 w-full rounded-md bg-bg-surface px-3 text-sm text-text-primary outline-none focus-visible:outline-2 focus-visible:outline-brand-primary" /> : null}
          {(!currentSuggestions || currentSuggestions.loading) ? <p role="status" className="px-3 py-2 text-xs text-text-secondary">{t("loadingSuggestions")}</p> : null}
          {currentSuggestions?.error ? <p role="alert" className="flex items-center justify-between px-3 text-xs text-status-error">{t("suggestionsError")}<button type="button" onClick={() => setRetry((value) => value + 1)} className={controlClass}>{t("retryAttachment")}</button></p> : null}
          {currentSuggestions && !currentSuggestions.loading && !currentSuggestions.error && !currentSuggestions.items.length ? <p className="px-3 py-2 text-xs text-text-secondary">{t("noSuggestions")}</p> : null}
          <ul id={listId} role="listbox" aria-label={t("suggestions")} className="max-h-48 overflow-y-auto">
            {currentSuggestions?.items.map((item, index) => <li key={item.id} role="presentation">
              <button id={`${listId}-${index}`} type="button" role="option" aria-label={item.label} aria-description={item.description} aria-selected={active === index} onMouseDown={(event) => event.preventDefault()} onPointerMove={() => setActive(index)} onClick={() => choose(item)} className={`flex min-h-11 w-full items-center gap-3 rounded-md px-3 text-left focus-visible:outline-2 focus-visible:outline-brand-primary ${active === index ? "bg-bg-surface text-text-primary" : "text-text-secondary hover:bg-bg-surface"}`}>
                <span className="shrink-0 text-sm">{item.label}</span><span className="truncate text-xs text-text-secondary">{item.description}</span>
              </button>
            </li>)}
          </ul>
          {currentSuggestions?.truncated ? <p className="px-3 py-1 text-xs text-text-secondary">{t("moreSuggestions")}</p> : null}
        </div> : null}
        <textarea ref={textarea} aria-label={t("placeholder")} aria-autocomplete="list" aria-controls={showSuggestions ? listId : undefined} aria-activedescendant={showSuggestions && currentSuggestions?.items[active] ? `${listId}-${active}` : undefined} title={t("draftHint")} placeholder={t("placeholder")} rows={2} value={composer.text} disabled={!canEdit} onChange={(event) => {
          composer.setText(event.target.value);
          setCaret(event.target.selectionStart);
          setSkillPicker(false);
        }} onSelect={(event) => setCaret(event.currentTarget.selectionStart)} onPaste={(event) => {
          const files = Array.from(event.clipboardData.files);
          if (!capabilities?.attachments || !files.length) return;
          event.preventDefault();
          composer.addFiles(files);
        }} onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (handleCompletionKey(event)) return;
          if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submit(); }
        }} className="block max-h-48 min-h-16 w-full min-w-0 resize-y bg-transparent px-2 py-1 text-sm leading-6 text-text-primary outline-none placeholder:text-text-secondary" />
        {(composer.error || chat.actionError) ? <p role="alert" className="px-2 py-1 text-xs text-status-error">{chat.actionError || t(composer.error ?? "sendError")}</p> : null}
        {chat.snapshot?.canSend && !inputReady ? <p role="status" className="px-2 py-1 text-xs text-text-secondary">{t(chat.snapshot.inputState === "occupied" ? "inputOccupied" : "inputWaiting")}</p> : null}
        <input ref={fileInput} type="file" multiple hidden aria-label={t("attachFiles")} onChange={(event) => { composer.addFiles(Array.from(event.target.files ?? [])); event.target.value = ""; }} />
        <input ref={photoInput} type="file" accept="image/*" multiple hidden aria-label={t("attachPhotos")} onChange={(event) => { composer.addFiles(Array.from(event.target.files ?? [])); event.target.value = ""; }} />
        <div data-testid="pane-chat-composer-tools" className="mt-1 flex flex-wrap items-center gap-1">
          <button type="button" disabled={!canEdit || !capabilities?.attachments} onClick={() => fileInput.current?.click()} className={controlClass}>{t("attachFiles")}</button>
          <button type="button" disabled={!canEdit || !capabilities?.attachments} onClick={() => photoInput.current?.click()} className={controlClass}>{t("attachPhotos")}</button>
          <button type="button" aria-expanded={showSuggestions && completion?.kind === "skill"} disabled={!canEdit || !capabilities?.skills || controlsOpen} onClick={() => { setSkillPicker((value) => !value); setSkillQuery(""); setDismissed(null); }} className={controlClass}>{t("skills")}</button>
          <button type="button" disabled={disabled || !chat.snapshot?.modelPicker.supported || !inputReady || controlsOpen} onClick={onModel} className={controlClass}>{t("model")}</button>
          <button type="button" disabled={disabled} aria-expanded={controlsOpen} onClick={onControls} className={controlClass}>{t("controls")}</button>
          <button type="button" disabled={disabled} onClick={() => void chat.interrupt()} className={controlClass}>{t("stop")}</button>
          <button type="submit" disabled={disabled || composer.sending || hasPending || (!composer.text.trim() && !composer.attachments.length) || controlsOpen || !inputReady} className="ml-auto min-h-10 shrink-0 rounded-lg bg-brand-primary px-4 text-xs font-medium text-white hover:bg-brand-active focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-primary disabled:opacity-40">{composer.sending ? t("sending") : t("send")}</button>
        </div>
      </div>
    </form>
  );
}
