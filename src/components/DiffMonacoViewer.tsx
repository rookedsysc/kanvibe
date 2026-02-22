"use client";

import { useMemo } from "react";
import { DiffEditor } from "@monaco-editor/react";

interface DiffMonacoViewerProps {
  originalContent: string;
  modifiedContent: string;
  filePath: string;
  language?: string;
}

/** 파일 확장자에서 Monaco 에디터 언어를 추론한다 */
function detectLanguage(filePath: string): string {
  const ext = filePath.split(".").pop()?.toLowerCase();
  const languageMap: Record<string, string> = {
    ts: "typescript",
    tsx: "typescriptreact",
    js: "javascript",
    jsx: "javascriptreact",
    py: "python",
    rb: "ruby",
    go: "go",
    rs: "rust",
    java: "java",
    kt: "kotlin",
    swift: "swift",
    c: "c",
    cpp: "cpp",
    h: "c",
    hpp: "cpp",
    cs: "csharp",
    php: "php",
    html: "html",
    css: "css",
    scss: "scss",
    less: "less",
    json: "json",
    yaml: "yaml",
    yml: "yaml",
    xml: "xml",
    md: "markdown",
    sql: "sql",
    sh: "shell",
    bash: "shell",
    zsh: "shell",
    dockerfile: "dockerfile",
    toml: "toml",
    ini: "ini",
    env: "plaintext",
    txt: "plaintext",
    dart: "dart",
    vue: "html",
    svelte: "html",
  };

  return languageMap[ext ?? ""] ?? "plaintext";
}

export { detectLanguage };

export default function DiffMonacoViewer({
  originalContent,
  modifiedContent,
  filePath,
  language,
}: DiffMonacoViewerProps) {
  const resolvedLanguage = useMemo(
    () => language ?? detectLanguage(filePath),
    [language, filePath]
  );

  return (
    <DiffEditor
      original={originalContent}
      modified={modifiedContent}
      language={resolvedLanguage}
      theme="light"
      options={{
        readOnly: true,
        renderSideBySide: true,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        fontSize: 13,
        lineNumbers: "on",
        wordWrap: "off",
        renderOverviewRuler: false,
      }}
    />
  );
}
