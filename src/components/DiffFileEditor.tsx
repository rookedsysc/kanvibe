"use client";

import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import Editor, { type OnMount } from "@monaco-editor/react";
import { detectLanguage } from "./DiffMonacoViewer";

type SaveStatus = "idle" | "saving" | "saved" | "error";

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
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const resolvedLanguage = useMemo(
    () => language ?? detectLanguage(filePath),
    [language, filePath]
  );

  const handleSave = useCallback(async () => {
    if (!editorRef.current) return;

    const currentContent = editorRef.current.getValue();
    setSaveStatus("saving");

    try {
      await onSave(currentContent);
      setSaveStatus("saved");

      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      saveTimerRef.current = setTimeout(() => setSaveStatus("idle"), 2000);
    } catch {
      setSaveStatus("error");
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      saveTimerRef.current = setTimeout(() => setSaveStatus("idle"), 3000);
    }
  }, [onSave]);

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
      case "error":
        return labels.saveError;
      default:
        return labels.save;
    }
  })();

  const saveButtonStyle = (() => {
    switch (saveStatus) {
      case "saved":
        return "bg-green-600 text-white";
      case "error":
        return "bg-red-500 text-white";
      case "saving":
        return "bg-brand-primary/70 text-white cursor-wait";
      default:
        return "bg-brand-primary text-white hover:bg-brand-primary/90";
    }
  })();

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-4 py-2 bg-bg-surface border-b border-border-default">
        <span className="text-xs text-text-secondary font-mono truncate">
          {filePath}
        </span>
        <button
          onClick={handleSave}
          disabled={saveStatus === "saving"}
          className={`px-3 py-1 text-xs rounded-md transition-colors ${saveButtonStyle}`}
        >
          {saveButtonText}
        </button>
      </div>
      <div className="flex-1 min-h-0">
        <Editor
          defaultValue={content}
          language={resolvedLanguage}
          theme="light"
          onMount={handleEditorMount}
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
