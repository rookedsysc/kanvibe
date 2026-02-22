"use client";

import { useState, useCallback, useMemo, useRef, useEffect } from "react";
import dynamic from "next/dynamic";
import { useTranslations } from "next-intl";
import type { DiffFile } from "@/app/actions/diff";
import {
  getOriginalFileContent,
  getFileContent,
  saveFileContent,
} from "@/app/actions/diff";
import DiffFileTree from "./DiffFileTree";

/** Monaco 에디터는 SSR 미지원이므로 dynamic import한다 */
const DiffMonacoViewer = dynamic(() => import("./DiffMonacoViewer"), {
  ssr: false,
  loading: () => (
    <div className="flex-1 flex items-center justify-center bg-bg-page">
      <div className="text-text-muted text-sm">Loading editor...</div>
    </div>
  ),
});

const DiffFileEditor = dynamic(() => import("./DiffFileEditor"), {
  ssr: false,
  loading: () => (
    <div className="flex-1 flex items-center justify-center bg-bg-page">
      <div className="text-text-muted text-sm">Loading editor...</div>
    </div>
  ),
});

type ViewMode = "diff" | "edit";

interface DiffPageClientProps {
  taskId: string;
  files: DiffFile[];
}

interface FileContentState {
  original: string;
  modified: string;
  loading: boolean;
}

/** GitHub 스타일 변경량 도트를 표시한다. 최대 5개의 도트로 추가/삭제 비율을 시각화한다 */
function ChangeStatDots({ additions, deletions }: { additions: number; deletions: number }) {
  const total = additions + deletions;
  if (total === 0) return null;

  const MAX_DOTS = 5;
  const addDots = Math.round((additions / total) * MAX_DOTS);
  const delDots = MAX_DOTS - addDots;

  return (
    <span className="flex gap-0.5 ml-1">
      {Array.from({ length: addDots }, (_, i) => (
        <span key={`a${i}`} className="w-2 h-2 rounded-sm bg-green-500" />
      ))}
      {Array.from({ length: delDots }, (_, i) => (
        <span key={`d${i}`} className="w-2 h-2 rounded-sm bg-red-500" />
      ))}
    </span>
  );
}

const MIN_SIDEBAR_WIDTH = 180;
const MAX_SIDEBAR_WIDTH = 500;
const DEFAULT_SIDEBAR_WIDTH = 288;

export default function DiffPageClient({ taskId, files }: DiffPageClientProps) {
  const t = useTranslations("diffView");
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("diff");
  const [viewedFiles, setViewedFiles] = useState<Set<string>>(new Set());
  const [fileContent, setFileContent] = useState<FileContentState>({
    original: "",
    modified: "",
    loading: false,
  });
  const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_SIDEBAR_WIDTH);
  const isResizing = useRef(false);

  /** 전체 파일의 총 변경 라인 수를 계산한다 */
  const totalStats = useMemo(() => {
    return files.reduce(
      (acc, file) => ({
        additions: acc.additions + file.additions,
        deletions: acc.deletions + file.deletions,
      }),
      { additions: 0, deletions: 0 }
    );
  }, [files]);

  const handleSelectFile = useCallback(
    async (filePath: string) => {
      setSelectedFile(filePath);
      setViewMode("diff");
      setFileContent({ original: "", modified: "", loading: true });

      const [original, modified] = await Promise.all([
        getOriginalFileContent(taskId, filePath),
        getFileContent(taskId, filePath),
      ]);

      setFileContent({ original, modified, loading: false });
    },
    [taskId]
  );

  /** 페이지 진입 시 첫 번째 파일을 자동 선택한다 */
  useEffect(() => {
    if (files.length > 0 && !selectedFile) {
      handleSelectFile(files[0].path);
    }
  }, [files, selectedFile, handleSelectFile]);

  const handleSave = useCallback(
    async (content: string) => {
      if (!selectedFile) return;
      const result = await saveFileContent(taskId, selectedFile, content);
      if (!result.success) {
        throw new Error(result.error);
      }
      /** 저장 후 modified 내용을 갱신한다 */
      setFileContent((prev) => ({ ...prev, modified: content }));
    },
    [taskId, selectedFile]
  );

  const toggleViewed = useCallback((filePath: string) => {
    setViewedFiles((prev) => {
      const next = new Set(prev);
      if (next.has(filePath)) {
        next.delete(filePath);
      } else {
        next.add(filePath);
      }
      return next;
    });
  }, []);

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isResizing.current = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;

    const handleMouseMove = (moveEvent: MouseEvent) => {
      if (!isResizing.current) return;
      const newWidth = startWidth + (moveEvent.clientX - startX);
      setSidebarWidth(Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, newWidth)));
    };

    const handleMouseUp = () => {
      isResizing.current = false;
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, [sidebarWidth]);

  const selectedFileData = files.find((f) => f.path === selectedFile);
  const isDeletedFile = selectedFileData?.status === "deleted";

  return (
    <div className="flex flex-1 min-h-0 gap-0">
      {/* 왼쪽 사이드바 - 변경 파일 트리 */}
      <aside
        className="shrink-0 bg-bg-surface border-r border-border-default flex flex-col"
        style={{ width: `${sidebarWidth}px` }}
      >
        <div className="px-3 py-2.5 border-b border-border-default flex items-center justify-between">
          <div>
            <h3 className="text-xs font-semibold text-text-primary">
              {t("changedFiles")}
            </h3>
            <div className="flex items-center gap-2 mt-0.5">
              <span className="text-[10px] text-text-muted">
                {t("fileCount", { count: files.length })}
              </span>
              {(totalStats.additions > 0 || totalStats.deletions > 0) && (
                <span className="text-[10px]">
                  <span className="text-green-600 font-medium">+{totalStats.additions}</span>
                  {" "}
                  <span className="text-red-500 font-medium">-{totalStats.deletions}</span>
                </span>
              )}
            </div>
          </div>
          {/* Viewed 진행률 */}
          {files.length > 0 && (
            <span className="text-[10px] text-text-muted whitespace-nowrap">
              {viewedFiles.size} / {files.length}
            </span>
          )}
        </div>
        <div className="flex-1 overflow-y-auto p-1.5">
          {files.length === 0 ? (
            <p className="text-xs text-text-muted px-2 py-4 text-center">
              {t("noChanges")}
            </p>
          ) : (
            <DiffFileTree
              files={files}
              selectedFile={selectedFile}
              onSelectFile={handleSelectFile}
              viewedFiles={viewedFiles}
            />
          )}
        </div>
      </aside>

      {/* 사이드바 리사이즈 핸들 */}
      <div
        onMouseDown={handleResizeStart}
        className="w-1 shrink-0 cursor-col-resize bg-transparent hover:bg-brand-primary/30 active:bg-brand-primary/50 transition-colors"
      />

      {/* 오른쪽 메인 - Diff/Editor 뷰 */}
      <main className="flex-1 flex flex-col min-h-0 min-w-0">
        {selectedFile && selectedFileData ? (
          <>
            {/* GitHub 스타일 파일 헤더 바 */}
            <div className="flex items-center gap-3 px-4 py-2 bg-bg-surface border-b border-border-default shrink-0">
              {/* 모드 전환 탭 */}
              <div className="flex bg-bg-page rounded-md border border-border-default p-0.5">
                <button
                  onClick={() => setViewMode("diff")}
                  className={`px-2.5 py-1 text-xs rounded transition-colors ${
                    viewMode === "diff"
                      ? "bg-bg-surface text-text-primary font-medium shadow-xs"
                      : "text-text-muted hover:text-text-secondary"
                  }`}
                >
                  {t("diffMode")}
                </button>
                {!isDeletedFile && (
                  <button
                    onClick={() => setViewMode("edit")}
                    className={`px-2.5 py-1 text-xs rounded transition-colors ${
                      viewMode === "edit"
                        ? "bg-bg-surface text-text-primary font-medium shadow-xs"
                        : "text-text-muted hover:text-text-secondary"
                    }`}
                  >
                    {t("editMode")}
                  </button>
                )}
              </div>

              {/* 파일 경로 */}
              <span className="text-xs font-mono text-text-secondary truncate flex-1">
                {selectedFile}
              </span>

              {/* 변경 통계 */}
              <span className="flex items-center gap-1.5 shrink-0 text-xs font-mono">
                <span className="text-green-600 font-semibold">+{selectedFileData.additions}</span>
                <span className="text-red-500 font-semibold">-{selectedFileData.deletions}</span>
                <ChangeStatDots
                  additions={selectedFileData.additions}
                  deletions={selectedFileData.deletions}
                />
              </span>

              {/* Viewed 체크박스 */}
              <label className="flex items-center gap-1.5 shrink-0 cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={viewedFiles.has(selectedFile)}
                  onChange={() => toggleViewed(selectedFile)}
                  className="w-3.5 h-3.5 rounded border-border-default text-brand-primary focus:ring-brand-primary cursor-pointer accent-[var(--color-brand-primary)]"
                />
                <span className="text-xs text-text-muted">
                  {viewedFiles.has(selectedFile) ? t("viewed") : t("markViewed")}
                </span>
              </label>
            </div>

            {/* 에디터 영역 */}
            <div className="flex-1 min-h-0">
              {fileContent.loading ? (
                <div className="flex-1 flex items-center justify-center h-full bg-bg-page">
                  <div className="text-text-muted text-sm">Loading...</div>
                </div>
              ) : viewMode === "diff" ? (
                <DiffMonacoViewer
                  originalContent={fileContent.original}
                  modifiedContent={fileContent.modified}
                  filePath={selectedFile}
                />
              ) : (
                <DiffFileEditor
                  content={fileContent.modified}
                  filePath={selectedFile}
                  onSave={handleSave}
                  labels={{
                    save: t("save"),
                    saving: t("saving"),
                    saved: t("saved"),
                    saveError: t("saveError"),
                  }}
                />
              )}
            </div>
          </>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center bg-bg-page gap-3">
            <svg width="48" height="48" viewBox="0 0 48 48" fill="none" className="text-text-muted opacity-30">
              <path
                d="M12 8h16l8 8v24a4 4 0 01-4 4H12a4 4 0 01-4-4V12a4 4 0 014-4z"
                stroke="currentColor"
                strokeWidth="2"
                fill="none"
              />
              <path d="M28 8v8h8" stroke="currentColor" strokeWidth="2" fill="none" />
              <path d="M16 24h16M16 30h12" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
            </svg>
            <p className="text-text-muted text-sm">
              {files.length > 0
                ? t("changedFiles")
                : t("noChanges")}
            </p>
          </div>
        )}
      </main>
    </div>
  );
}
