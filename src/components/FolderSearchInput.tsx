"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { useTranslations } from "next-intl";

interface FolderSearchInputProps {
  onSelect: (path: string) => void;
  sshHost?: string;
  name: string;
  placeholder?: string;
}

interface FuzzyMatch {
  path: string;
  score: number;
  /** 매칭된 문자의 인덱스 배열 */
  matchedIndices: number[];
}

/** fzf 스타일 subsequence fuzzy matching을 수행한다 */
function fuzzyMatch(query: string, target: string): FuzzyMatch | null {
  const lowerQuery = query.toLowerCase();
  const lowerTarget = target.toLowerCase();
  const matchedIndices: number[] = [];

  let queryIdx = 0;
  for (let i = 0; i < lowerTarget.length && queryIdx < lowerQuery.length; i++) {
    if (lowerTarget[i] === lowerQuery[queryIdx]) {
      matchedIndices.push(i);
      queryIdx++;
    }
  }

  if (queryIdx !== lowerQuery.length) return null;

  /** 점수 계산: 연속 매칭 보너스 + 경로 끝부분(폴더명) 매칭 가중치 */
  let score = 0;
  const lastSlash = target.lastIndexOf("/");

  for (let i = 0; i < matchedIndices.length; i++) {
    const idx = matchedIndices[i];
    if (idx > lastSlash) score += 2;
    else score += 1;

    if (i > 0 && matchedIndices[i] === matchedIndices[i - 1] + 1) {
      score += 3;
    }
  }

  return { path: target, score, matchedIndices };
}

/** 매칭된 문자를 하이라이트하여 렌더링한다 */
function HighlightedPath({ path, matchedIndices }: { path: string; matchedIndices: number[] }) {
  const matchSet = new Set(matchedIndices);
  const segments: { text: string; highlighted: boolean }[] = [];
  let current = "";
  let currentHighlighted = false;

  for (let i = 0; i < path.length; i++) {
    const isMatch = matchSet.has(i);
    if (i === 0) {
      currentHighlighted = isMatch;
      current = path[i];
    } else if (isMatch === currentHighlighted) {
      current += path[i];
    } else {
      segments.push({ text: current, highlighted: currentHighlighted });
      current = path[i];
      currentHighlighted = isMatch;
    }
  }
  if (current) segments.push({ text: current, highlighted: currentHighlighted });

  return (
    <span className="font-mono text-xs">
      {segments.map((seg, i) =>
        seg.highlighted ? (
          <span key={i} className="text-brand-primary font-bold">{seg.text}</span>
        ) : (
          <span key={i}>{seg.text}</span>
        )
      )}
    </span>
  );
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
  const [directories, setDirectories] = useState<string[]>([]);
  const [filteredDirs, setFilteredDirs] = useState<FuzzyMatch[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isOpen, setIsOpen] = useState(false);
  const [isLoading, setIsLoading] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // inputValue에서 탐색할 부모 경로와 검색어를 파싱한다
  const lastSlash = inputValue.lastIndexOf("/");
  const parentPath = lastSlash > 0 ? inputValue.substring(0, lastSlash) : "~";
  const searchTerm = lastSlash >= 0 ? inputValue.substring(lastSlash + 1) : "";

  /** 지정 경로의 하위 디렉토리를 API에서 가져온다 */
  const fetchDirectories = useCallback(async (dirPath: string) => {
    setIsLoading(true);
    try {
      const params = new URLSearchParams({ path: dirPath });
      if (sshHost) params.set("sshHost", sshHost);
      const res = await fetch(`/api/directories?${params}`);
      const result: string[] = await res.json();
      setDirectories(result);
    } catch {
      setDirectories([]);
    } finally {
      setIsLoading(false);
    }
  }, [sshHost]);

  /** parentPath 변경 시 디렉토리 목록을 다시 가져온다 */
  useEffect(() => {
    fetchDirectories(parentPath);
  }, [parentPath, fetchDirectories]);

  /** searchTerm 또는 directories 변경 시 fuzzy 필터링을 수행한다 */
  useEffect(() => {
    if (!searchTerm) {
      setFilteredDirs(directories.map((p) => ({ path: p, score: 0, matchedIndices: [] })));
      setSelectedIndex(0);
      return;
    }

    const matches = directories
      .map((dirName) => fuzzyMatch(searchTerm, dirName))
      .filter((m): m is FuzzyMatch => m !== null)
      .sort((a, b) => b.score - a.score);

    setFilteredDirs(matches);
    setSelectedIndex(0);
  }, [searchTerm, directories]);

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
    const fullPath = parentPath === "~" ? `~/${dirName}` : `${parentPath}/${dirName}`;
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
          const newPath = parentPath === "~" ? `~/${dirName}/` : `${parentPath}/${dirName}/`;
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
      <input type="hidden" name={name} value={selectedPath} />

      {isOpen && (
        <div className="absolute z-50 left-0 right-0 mt-1 bg-bg-surface border border-border-default rounded-md shadow-lg max-h-60 overflow-y-auto">
          {/* 현재 경로 breadcrumb */}
          <div className="px-3 py-1.5 text-[10px] text-text-muted font-mono border-b border-border-default">
            {parentPath}/
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
                    <HighlightedPath path={match.path} matchedIndices={match.matchedIndices} />
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
