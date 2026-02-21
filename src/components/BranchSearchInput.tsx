"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { useTranslations } from "next-intl";
import { fuzzyMatch, type FuzzyMatch } from "@/utils/fuzzySearch";
import HighlightedText from "@/components/HighlightedText";

interface BranchSearchInputProps {
  branches: string[];
  value: string;
  onChange: (branch: string) => void;
  placeholder?: string;
}

/** 브랜치 목록에서 fuzzy 검색으로 선택할 수 있는 입력 컴포넌트 */
export default function BranchSearchInput({
  branches,
  value,
  onChange,
  placeholder,
}: BranchSearchInputProps) {
  const t = useTranslations("task");
  const [inputValue, setInputValue] = useState(value);
  const [filteredBranches, setFilteredBranches] = useState<FuzzyMatch[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isOpen, setIsOpen] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  /** 외부에서 value가 변경되면 input을 동기화한다 */
  useEffect(() => {
    setInputValue(value);
  }, [value]);

  /** 검색어 변경 시 fuzzy 필터링을 수행한다 */
  useEffect(() => {
    if (!inputValue) {
      setFilteredBranches(
        branches.map((b) => ({ path: b, score: 0, matchedIndices: [] }))
      );
      setSelectedIndex(0);
      return;
    }

    const matches = branches
      .map((branch) => fuzzyMatch(inputValue, branch))
      .filter((m): m is FuzzyMatch => m !== null)
      .sort((a, b) => b.score - a.score);

    setFilteredBranches(matches);
    setSelectedIndex(0);
  }, [inputValue, branches]);

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
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        setIsOpen(false);
        /** 입력값이 유효한 브랜치가 아니면 이전 선택값으로 복원한다 */
        if (!branches.includes(inputValue)) {
          setInputValue(value);
        }
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [branches, inputValue, value]);

  const handleSelect = useCallback(
    (branch: string) => {
      setInputValue(branch);
      setIsOpen(false);
      onChange(branch);
    },
    [onChange]
  );

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
        setSelectedIndex((prev) =>
          Math.min(prev + 1, filteredBranches.length - 1)
        );
        break;
      case "ArrowUp":
        e.preventDefault();
        setSelectedIndex((prev) => Math.max(prev - 1, 0));
        break;
      case "Enter":
        e.preventDefault();
        if (filteredBranches[selectedIndex]) {
          handleSelect(filteredBranches[selectedIndex].path);
        }
        break;
      case "Escape":
        e.preventDefault();
        setIsOpen(false);
        setInputValue(value);
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
          setIsOpen(true);
        }}
        onFocus={() => setIsOpen(true)}
        onKeyDown={handleKeyDown}
        placeholder={placeholder || t("branchSearchPlaceholder")}
        className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
        autoComplete="off"
      />

      {isOpen && (
        <div className="absolute z-50 left-0 right-0 mt-1 bg-bg-surface border border-border-default rounded-md shadow-lg max-h-60 overflow-y-auto">
          {filteredBranches.length === 0 ? (
            <div className="px-3 py-2 text-xs text-text-muted">
              {t("noBranchesFound")}
            </div>
          ) : (
            <>
              <div className="px-3 py-1 text-[10px] text-text-muted">
                {t("branchCount", { count: filteredBranches.length })}
              </div>
              <div ref={listRef}>
                {filteredBranches.map((match, index) => (
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
                    <HighlightedText
                      text={match.path}
                      matchedIndices={match.matchedIndices}
                    />
                  </button>
                ))}
              </div>
            </>
          )}

          {/* 네비게이션 힌트 */}
          <div className="px-3 py-1.5 text-[10px] text-text-muted border-t border-border-default">
            {t("branchNavHint")}
          </div>
        </div>
      )}
    </div>
  );
}
