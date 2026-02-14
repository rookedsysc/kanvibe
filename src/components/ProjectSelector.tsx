"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import type { Project } from "@/entities/Project";

type BaseProps = {
  projects: Project[];
  placeholder?: string;
  searchPlaceholder?: string;
  compact?: boolean;
};

type SingleSelectProps = BaseProps & {
  multiple?: false;
  selectedProjectId: string;
  onSelect: (projectId: string) => void;
  /** "전체 프로젝트" 등 전체 선택 옵션. label이 드롭다운에 표시된다 */
  allOption?: { label: string };
};

type MultiSelectProps = BaseProps & {
  multiple: true;
  selectedProjectIds: string[];
  onSelectionChange: (projectIds: string[]) => void;
};

type ProjectSelectorProps = SingleSelectProps | MultiSelectProps;

export default function ProjectSelector(props: ProjectSelectorProps) {
  const {
    projects,
    placeholder = "",
    searchPlaceholder = "",
    compact = false,
  } = props;

  const isMultiple = props.multiple === true;

  const selectedProjectIds = isMultiple
    ? (props as MultiSelectProps).selectedProjectIds
    : [];
  const onSelectionChange = isMultiple
    ? (props as MultiSelectProps).onSelectionChange
    : undefined;
  const selectedProjectId = !isMultiple
    ? (props as SingleSelectProps).selectedProjectId
    : "";
  const onSelect = !isMultiple
    ? (props as SingleSelectProps).onSelect
    : undefined;
  const allOption = !isMultiple
    ? (props as SingleSelectProps).allOption
    : undefined;

  const [searchQuery, setSearchQuery] = useState("");
  const [isOpen, setIsOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const isComposingRef = useRef(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const triggerInputRef = useRef<HTMLInputElement>(null);

  const filteredProjects = searchQuery
    ? projects.filter((p) =>
        p.name.toLowerCase().includes(searchQuery.toLowerCase())
      )
    : projects;

  /** 단일 선택 모드에서 "전체" 옵션은 검색 중이 아닐 때만 표시한다 */
  const showAllOption = !isMultiple && !!allOption && !searchQuery;

  const singleDisplayText = (() => {
    if (isMultiple) return "";
    if (!selectedProjectId && allOption) return allOption.label;
    const selected = projects.find((p) => p.id === selectedProjectId);
    if (!selected) return "";
    return selected.name + (selected.sshHost ? ` (${selected.sshHost})` : "");
  })();

  const selectedProjects = isMultiple
    ? projects.filter((p) => selectedProjectIds.includes(p.id))
    : [];

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        setIsOpen(false);
        setSearchQuery("");
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  /** 멀티 선택 드롭다운이 열리면 검색 input에 포커스한다 */
  useEffect(() => {
    if (isOpen && isMultiple && searchInputRef.current) {
      searchInputRef.current.focus();
    }
  }, [isOpen, isMultiple]);

  const handleToggle = useCallback(
    (projectId: string) => {
      if (isMultiple && onSelectionChange) {
        const nextIds = selectedProjectIds.includes(projectId)
          ? selectedProjectIds.filter((id) => id !== projectId)
          : [...selectedProjectIds, projectId];
        onSelectionChange(nextIds);
      } else if (onSelect) {
        onSelect(projectId);
        setSearchQuery("");
        setIsOpen(false);
        setHighlightedIndex(-1);
      }
    },
    [isMultiple, selectedProjectIds, onSelectionChange, onSelect]
  );

  const handleSelectAll = useCallback(() => {
    if (onSelect) {
      onSelect("");
      setSearchQuery("");
      setIsOpen(false);
      setHighlightedIndex(-1);
    }
  }, [onSelect]);

  const removeChip = useCallback(
    (projectId: string, e: React.MouseEvent) => {
      e.stopPropagation();
      if (onSelectionChange) {
        onSelectionChange(selectedProjectIds.filter((id) => id !== projectId));
      }
    },
    [selectedProjectIds, onSelectionChange]
  );

  const handleSearchInput = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (isComposingRef.current) return;
      setSearchQuery(e.target.value);
      setHighlightedIndex(0);
    },
    []
  );

  const handleCompositionStart = useCallback(() => {
    isComposingRef.current = true;
  }, []);

  const handleCompositionEnd = useCallback(
    (e: React.CompositionEvent<HTMLInputElement>) => {
      isComposingRef.current = false;
      setSearchQuery(e.currentTarget.value);
      setHighlightedIndex(0);
    },
    []
  );

  // ===== 멀티 선택 모드 =====
  if (isMultiple) {
    const handleMultiKeyDown = (e: React.KeyboardEvent) => {
      if (!isOpen) {
        if (e.key === "ArrowDown" || e.key === "Enter") {
          e.preventDefault();
          setIsOpen(true);
        }
        return;
      }

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setHighlightedIndex((prev) =>
            prev < filteredProjects.length - 1 ? prev + 1 : 0
          );
          break;
        case "ArrowUp":
          e.preventDefault();
          setHighlightedIndex((prev) =>
            prev > 0 ? prev - 1 : filteredProjects.length - 1
          );
          break;
        case "Enter":
          e.preventDefault();
          if (
            highlightedIndex >= 0 &&
            highlightedIndex < filteredProjects.length
          ) {
            handleToggle(filteredProjects[highlightedIndex].id);
          }
          break;
        case "Escape":
          e.preventDefault();
          setIsOpen(false);
          setSearchQuery("");
          setHighlightedIndex(-1);
          break;
      }
    };

    return (
      <div
        ref={containerRef}
        className="relative"
        onKeyDown={handleMultiKeyDown}
      >
        {/* 트리거: 선택된 프로젝트 칩 또는 placeholder */}
        <div
          onClick={() => setIsOpen(!isOpen)}
          className={`w-full px-2 bg-bg-page border rounded-md text-text-primary cursor-pointer flex items-center gap-1 flex-wrap ${
            compact ? "py-1 min-h-[34px]" : "py-1.5 min-h-[38px]"
          } ${isOpen ? "border-brand-primary" : "border-border-default"}`}
        >
          {selectedProjects.length === 0 ? (
            <span className="text-text-muted text-sm px-1">{placeholder}</span>
          ) : (
            selectedProjects.map((p) => (
              <span
                key={p.id}
                className="inline-flex items-center gap-0.5 px-1.5 py-0.5 text-xs bg-brand-primary/10 text-brand-primary rounded font-medium max-w-[120px]"
              >
                <span className="truncate">{p.name}</span>
                <button
                  type="button"
                  onMouseDown={(e) => removeChip(p.id, e)}
                  className="ml-0.5 hover:text-status-error flex-shrink-0 leading-none"
                >
                  &times;
                </button>
              </span>
            ))
          )}
          <svg
            className="ml-auto flex-shrink-0 text-text-muted"
            width="12"
            height="12"
            viewBox="0 0 12 12"
            fill="none"
          >
            <path
              d="M3 5L6 8L9 5"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </div>

        {isOpen && (
          <div className="absolute z-50 mt-1 w-full bg-bg-surface border border-border-default rounded-md shadow-md">
            <div className="p-2 border-b border-border-default">
              <input
                ref={searchInputRef}
                type="text"
                value={searchQuery}
                onCompositionStart={handleCompositionStart}
                onCompositionEnd={handleCompositionEnd}
                onChange={handleSearchInput}
                placeholder={searchPlaceholder}
                className="w-full px-2 py-1 text-sm bg-bg-page border border-border-default rounded text-text-primary focus:outline-none focus:border-brand-primary"
              />
            </div>
            <ul className="max-h-48 overflow-y-auto">
              {filteredProjects.length === 0 ? (
                <li className="px-3 py-2 text-sm text-text-muted">
                  {placeholder}
                </li>
              ) : (
                filteredProjects.map((project, index) => {
                  const checked = selectedProjectIds.includes(project.id);
                  return (
                    <li
                      key={project.id}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        handleToggle(project.id);
                      }}
                      onMouseEnter={() => setHighlightedIndex(index)}
                      className={`flex items-center gap-2 px-3 py-2 text-sm cursor-pointer transition-colors ${
                        index === highlightedIndex
                          ? "bg-brand-primary/10 text-text-primary"
                          : "text-text-primary hover:bg-bg-page"
                      }`}
                    >
                      <span
                        className={`w-4 h-4 flex items-center justify-center flex-shrink-0 rounded border transition-colors ${
                          checked
                            ? "bg-brand-primary border-brand-primary text-text-inverse"
                            : "border-border-default"
                        }`}
                      >
                        {checked && (
                          <svg
                            width="10"
                            height="8"
                            viewBox="0 0 10 8"
                            fill="none"
                          >
                            <path
                              d="M1 4L3.5 6.5L9 1"
                              stroke="currentColor"
                              strokeWidth="1.5"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            />
                          </svg>
                        )}
                      </span>
                      <span className="truncate">
                        {project.name}
                        {project.sshHost && (
                          <span className="text-text-muted ml-1">
                            ({project.sshHost})
                          </span>
                        )}
                      </span>
                    </li>
                  );
                })
              )}
            </ul>
          </div>
        )}
      </div>
    );
  }

  // ===== 단일 선택 모드 =====
  const singleTotalItems =
    (showAllOption ? 1 : 0) + filteredProjects.length;

  const handleSingleKeyDown = (e: React.KeyboardEvent) => {
    if (!isOpen) {
      if (e.key === "ArrowDown" || e.key === "Enter") {
        e.preventDefault();
        setIsOpen(true);
      }
      return;
    }

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setHighlightedIndex((prev) =>
          prev < singleTotalItems - 1 ? prev + 1 : 0
        );
        break;
      case "ArrowUp":
        e.preventDefault();
        setHighlightedIndex((prev) =>
          prev > 0 ? prev - 1 : singleTotalItems - 1
        );
        break;
      case "Enter":
        e.preventDefault();
        if (highlightedIndex >= 0 && highlightedIndex < singleTotalItems) {
          if (showAllOption && highlightedIndex === 0) {
            handleSelectAll();
          } else {
            const projectIndex = showAllOption
              ? highlightedIndex - 1
              : highlightedIndex;
            if (
              projectIndex >= 0 &&
              projectIndex < filteredProjects.length
            ) {
              handleToggle(filteredProjects[projectIndex].id);
            }
          }
        }
        break;
      case "Escape":
        e.preventDefault();
        setIsOpen(false);
        setSearchQuery("");
        setHighlightedIndex(-1);
        triggerInputRef.current?.blur();
        break;
    }
  };

  return (
    <div ref={containerRef} className="relative">
      <input
        ref={triggerInputRef}
        type="text"
        value={isOpen ? searchQuery : singleDisplayText}
        onCompositionStart={handleCompositionStart}
        onCompositionEnd={handleCompositionEnd}
        onChange={handleSearchInput}
        onFocus={() => {
          const selected = projects.find((p) => p.id === selectedProjectId);
          setSearchQuery(selected ? selected.name : "");
          setIsOpen(true);
          setHighlightedIndex(0);
          requestAnimationFrame(() => triggerInputRef.current?.select());
        }}
        onKeyDown={handleSingleKeyDown}
        placeholder={isOpen ? searchPlaceholder : placeholder}
        className={`w-full bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors cursor-pointer ${
          compact ? "px-3 py-1.5 text-sm" : "px-3 py-2"
        }`}
      />
      {isOpen && (
        <ul className="absolute z-50 mt-1 w-full max-h-48 overflow-y-auto bg-bg-surface border border-border-default rounded-md shadow-md">
          {showAllOption && (
            <li
              onMouseDown={(e) => {
                e.preventDefault();
                handleSelectAll();
              }}
              onMouseEnter={() => setHighlightedIndex(0)}
              className={`px-3 py-2 text-sm cursor-pointer transition-colors ${
                highlightedIndex === 0
                  ? "bg-brand-primary/10 text-text-primary"
                  : "text-text-primary hover:bg-bg-page"
              } ${!selectedProjectId ? "font-medium" : ""}`}
            >
              {allOption!.label}
            </li>
          )}
          {filteredProjects.length === 0 && !showAllOption ? (
            <li className="px-3 py-2 text-sm text-text-muted">
              {placeholder}
            </li>
          ) : (
            filteredProjects.map((project, index) => {
              const itemIndex = showAllOption ? index + 1 : index;
              return (
                <li
                  key={project.id}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    handleToggle(project.id);
                  }}
                  onMouseEnter={() => setHighlightedIndex(itemIndex)}
                  className={`px-3 py-2 text-sm cursor-pointer transition-colors ${
                    itemIndex === highlightedIndex
                      ? "bg-brand-primary/10 text-text-primary"
                      : "text-text-primary hover:bg-bg-page"
                  } ${project.id === selectedProjectId ? "font-medium" : ""}`}
                >
                  {project.name}
                  {project.sshHost && (
                    <span className="text-text-muted ml-1">
                      ({project.sshHost})
                    </span>
                  )}
                </li>
              );
            })
          )}
        </ul>
      )}
    </div>
  );
}
