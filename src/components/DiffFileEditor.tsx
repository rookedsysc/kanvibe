"use client";

import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import Editor, { type OnMount } from "@monaco-editor/react";
import { detectLanguage } from "./DiffMonacoViewer";

type SaveStatus = "idle" | "saving" | "saved";

interface DiffFileEditorProps {
  content: string;
  filePath: string;
  onSave: (content: string) => Promise<void>;
  language?: string;
  labels: {
    save: string;
    saving: string;
    saved: string;
    saveError: string;
    unsavedChanges: string;
  };
}

export default function DiffFileEditor({
  content,
  filePath,
  onSave,
  language,
  labels,
}: DiffFileEditorProps) {
  const [saveStatus, setSaveStatus] = useState<SaveStatus>("idle");
  /** 실패 사유는 다시 저장할 때까지 남긴다. 몇 초 뒤 지우면 무엇이 실패했는지 확인할 수 없다 */
  const [saveFailureReason, setSaveFailureReason] = useState<string | null>(null);
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** 버튼은 disabled로 막히지만 단축키는 handleSave를 바로 부르므로 같은 가드를 함께 쓴다 */
  const isSavingRef = useRef(false);
  /** 저장하는 동안 더 편집했으면 저장이 끝나도 미저장 표시를 지우지 않는다 */
  const editRevisionRef = useRef(0);

  const resolvedLanguage = useMemo(
    () => language ?? detectLanguage(filePath),
    [language, filePath]
  );

  const handleSave = useCallback(async () => {
    if (!editorRef.current || isSavingRef.current) return;

    isSavingRef.current = true;
    const currentContent = editorRef.current.getValue();
    const savingRevision = editRevisionRef.current;
    setSaveStatus("saving");
    setSaveFailureReason(null);
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);

    try {
      await onSave(currentContent);
      setSaveStatus("saved");
      if (editRevisionRef.current === savingRevision) {
        setHasUnsavedChanges(false);
      }
      saveTimerRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
    } catch (error) {
      setSaveStatus("idle");
      setSaveFailureReason(error instanceof Error ? error.message : String(error));
    } finally {
      isSavingRef.current = false;
    }
  }, [onSave]);

  const handleEditorChange = useCallback(() => {
    editRevisionRef.current += 1;
    setHasUnsavedChanges(true);
  }, []);

  const handleEditorMount: OnMount = useCallback(
    (editor) => {
      editorRef.current = editor;

      /** Ctrl/Cmd+S 단축키로 저장 */
      editor.addCommand(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (window as any).monaco?.KeyMod.CtrlCmd |
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (window as any).monaco?.KeyCode.KeyS,
        () => handleSave()
      );
    },
    [handleSave]
  );

  useEffect(() => {
    return () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    };
  }, []);

  const saveButtonText = (() => {
    switch (saveStatus) {
      case "saving":
        return labels.saving;
      case "saved":
        return labels.saved;
      default:
        return labels.save;
    }
  })();

  const saveButtonStyle = (() => {
    switch (saveStatus) {
      case "saved":
        return "bg-green-600 text-white";
      case "saving":
        return "bg-brand-primary/70 text-white cursor-wait";
      default:
        return "bg-brand-primary text-white hover:bg-brand-primary/90";
    }
  })();

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-4 py-2 bg-bg-surface border-b border-border-default">
        <span className="flex min-w-0 items-center gap-2 text-xs text-text-secondary font-mono">
          <span className="truncate">{filePath}</span>
          {hasUnsavedChanges ? (
            <span className="shrink-0 font-sans text-status-warning">{labels.unsavedChanges}</span>
          ) : null}
        </span>
        <button
          onClick={handleSave}
          disabled={saveStatus === "saving"}
          aria-busy={saveStatus === "saving" || undefined}
          className={`px-3 py-1 text-xs rounded-md transition-colors ${saveButtonStyle}`}
        >
          {saveButtonText}
        </button>
      </div>
      {saveFailureReason ? (
        <p role="alert" className="border-b border-status-error/20 bg-bg-page px-4 py-1.5 text-xs text-status-error">
          {labels.saveError}: {saveFailureReason}
        </p>
      ) : null}
      <div className="flex-1 min-h-0">
        <Editor
          defaultValue={content}
          language={resolvedLanguage}
          theme="light"
          onMount={handleEditorMount}
          onChange={handleEditorChange}
          options={{
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            fontSize: 13,
            lineNumbers: "on",
            wordWrap: "off",
            automaticLayout: true,
          }}
        />
      </div>
    </div>
  );
}
