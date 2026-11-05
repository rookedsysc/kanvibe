"use client";

import { useState, useRef, useEffect, useMemo } from "react";
import { useTranslations } from "next-intl";
import { fuzzyMatch, type FuzzyMatch } from "@/utils/fuzzySearch";
import HighlightedText from "@/components/HighlightedText";
import { listSubdirectories } from "@/desktop/renderer/actions/project";

interface FolderSearchInputProps {
  onSelect: (path: string) => void;
  sshHost?: string;
  name: string;
  placeholder?: string;
}


export default function FolderSearchInput({
  onSelect,
  sshHost,
  name,
  placeholder,
}: FolderSearchInputProps) {
  const t = useTranslations("settings");
  const [inputValue, setInputValue] = useState("~/");
  const [selectedPath, setSelectedPath] = useState("");
  const [directoryResult, setDirectoryResult] = useState<{
    parentPath: string;
    sshHost: string | undefined;
    directories: string[];
  } | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isOpen, setIsOpen] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // inputValue에서 탐색할 부모 경로와 검색어를 파싱한다
  const lastSlash = inputValue.lastIndexOf("/");
  const parentPath = lastSlash > 0 ? inputValue.substring(0, lastSlash) : lastSlash === 0 ? "/" : "~";
  const searchTerm = lastSlash >= 0 ? inputValue.substring(lastSlash + 1) : "";

  const isCurrentResult = directoryResult?.parentPath === parentPath && directoryResult.sshHost === sshHost;
  const isLoading = !isCurrentResult;

  /** 경로와 호스트가 바뀌면 이전 요청은 더 이상 이 입력의 결과를 변경하지 않는다 */
  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const directories = await listSubdirectories(parentPath, sshHost);
        if (active) setDirectoryResult({ parentPath, sshHost, directories });
      } catch {
        if (active) setDirectoryResult({ parentPath, sshHost, directories: [] });
      }
    })();
    return () => { active = false; };
  }, [parentPath, sshHost]);

  const filteredDirs = useMemo(() => {
    const directories = isCurrentResult ? directoryResult.directories : [];
    if (!searchTerm) {
      return directories.map((path) => ({ path, score: 0, matchedIndices: [] }));
    }
    return directories
      .map((dirName) => fuzzyMatch(searchTerm, dirName))
      .filter((match): match is FuzzyMatch => match !== null)
      .sort((a, b) => b.score - a.score);
  }, [directoryResult, isCurrentResult, searchTerm]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [filteredDirs]);

  /** 선택된 항목이 보이도록 스크롤한다 */
  useEffect(() => {
    if (!listRef.current) return;
    const items = listRef.current.children;
    if (items[selectedIndex]) {
      (items[selectedIndex] as HTMLElement).scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  /** 외부 클릭 시 드롭다운을 닫는다 */
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  /** 폴더를 선택하여 확정한다 */
  function handleSelect(dirName: string) {
    const fullPath = `${parentPath === "/" ? "" : parentPath}/${dirName}`;
    setSelectedPath(fullPath);
    setInputValue(fullPath);
    setIsOpen(false);
    onSelect(fullPath);
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (!isOpen) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        setIsOpen(true);
        e.preventDefault();
      }
      return;
    }

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setSelectedIndex((prev) => Math.min(prev + 1, filteredDirs.length - 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setSelectedIndex((prev) => Math.max(prev - 1, 0));
        break;
      case "Tab":
        e.preventDefault();
        if (filteredDirs[selectedIndex]) {
          // Tab으로 하위 폴더에 진입: 선택한 폴더명 뒤에 /를 붙여 하위 탐색
          const dirName = filteredDirs[selectedIndex].path;
          const newPath = `${parentPath === "/" ? "" : parentPath}/${dirName}/`;
          setInputValue(newPath);
          setSelectedIndex(0);
        }
        break;
      case "Enter":
        e.preventDefault();
        if (filteredDirs[selectedIndex]) {
          handleSelect(filteredDirs[selectedIndex].path);
        }
        break;
      case "Escape":
        e.preventDefault();
        setIsOpen(false);
        break;
    }
  }

  return (
    <div ref={containerRef} className="relative">
      <input
        ref={inputRef}
        type="text"
        value={inputValue}
        onChange={(e) => {
          setInputValue(e.target.value);
          setSelectedPath("");
          setIsOpen(true);
        }}
        onFocus={() => setIsOpen(true)}
        onKeyDown={handleKeyDown}
        placeholder={placeholder || t("searchFolderPlaceholder")}
        className="w-full px-3 py-1.5 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
        autoComplete="off"
      />
      {/* form 전송을 위한 hidden input */}
      <input type="hidden" name={name} value={selectedPath || inputValue} />

      {isOpen && (
        <div className="absolute z-50 left-0 right-0 mt-1 bg-bg-surface border border-border-default rounded-md shadow-lg max-h-60 overflow-y-auto">
          {/* 현재 경로 breadcrumb */}
          <div className="px-3 py-1.5 text-[10px] text-text-muted font-mono border-b border-border-default">
            {parentPath === "/" ? "/" : `${parentPath}/`}
          </div>

          {isLoading ? (
            <div className="px-3 py-2 text-xs text-text-muted">{t("loadingFolders")}</div>
          ) : filteredDirs.length === 0 ? (
            <div className="px-3 py-2 text-xs text-text-muted">{t("noFoldersFound")}</div>
          ) : (
            <>
              <div className="px-3 py-1 text-[10px] text-text-muted">
                {t("folderCount", { count: filteredDirs.length })}
              </div>
              <div ref={listRef}>
                {filteredDirs.map((match, index) => (
                  <button
                    key={match.path}
                    type="button"
                    className={`w-full text-left px-3 py-1.5 hover:bg-brand-primary/10 transition-colors ${
                      index === selectedIndex
                        ? "bg-brand-primary/10 text-text-primary"
                        : "text-text-secondary"
                    }`}
                    onClick={() => handleSelect(match.path)}
                    onMouseEnter={() => setSelectedIndex(index)}
                  >
                    <HighlightedText text={match.path} matchedIndices={match.matchedIndices} />
                  </button>
                ))}
              </div>
            </>
          )}

          {/* 네비게이션 힌트 */}
          <div className="px-3 py-1.5 text-[10px] text-text-muted border-t border-border-default">
            {t("folderNavHint")}
          </div>
        </div>
      )}
    </div>
  );
}
