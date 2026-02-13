"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import type { Project } from "@/entities/Project";

interface ProjectSelectorProps {
  projects: Project[];
  selectedProjectId: string;
  onSelect: (projectId: string) => void;
  placeholder?: string;
  searchPlaceholder?: string;
  /** "전체 프로젝트" 등 전체 선택 옵션. label이 드롭다운에 표시된다 */
  allOption?: { label: string };
  /** 헤더 등 좁은 영역에서 사용할 컴팩트 스타일 */
  compact?: boolean;
}

export default function ProjectSelector({
  projects,
  selectedProjectId,
  onSelect,
  placeholder = "",
  searchPlaceholder = "",
  allOption,
  compact = false,
}: ProjectSelectorProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [isOpen, setIsOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const filteredProjects = searchQuery
    ? projects.filter((p) =>
        p.name.toLowerCase().includes(searchQuery.toLowerCase())
      )
    : projects;

  /** "전체" 옵션은 검색 중이 아닐 때만 표시한다 */
  const showAllOption = !!allOption && !searchQuery;
  const totalItems = (showAllOption ? 1 : 0) + filteredProjects.length;

  const selectedDisplayText = (() => {
    if (!selectedProjectId && allOption) return allOption.label;
    const selected = projects.find((p) => p.id === selectedProjectId);
    if (!selected) return "";
    return selected.name + (selected.sshHost ? ` (${selected.sshHost})` : "");
  })();

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

  const selectItem = useCallback(
    (projectId: string) => {
      onSelect(projectId);
      setSearchQuery("");
      setIsOpen(false);
      setHighlightedIndex(-1);
    },
    [onSelect]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
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
            prev < totalItems - 1 ? prev + 1 : 0
          );
          break;
        case "ArrowUp":
          e.preventDefault();
          setHighlightedIndex((prev) =>
            prev > 0 ? prev - 1 : totalItems - 1
          );
          break;
        case "Enter":
          e.preventDefault();
          if (highlightedIndex >= 0 && highlightedIndex < totalItems) {
            if (showAllOption && highlightedIndex === 0) {
              selectItem("");
            } else {
              const projectIndex = showAllOption
                ? highlightedIndex - 1
                : highlightedIndex;
              if (
                projectIndex >= 0 &&
                projectIndex < filteredProjects.length
              ) {
                selectItem(filteredProjects[projectIndex].id);
              }
            }
          }
          break;
        case "Escape":
          e.preventDefault();
          setIsOpen(false);
          setSearchQuery("");
          setHighlightedIndex(-1);
          inputRef.current?.blur();
          break;
      }
    },
    [
      isOpen,
      highlightedIndex,
      totalItems,
      showAllOption,
      filteredProjects,
      selectItem,
    ]
  );

  return (
    <div ref={containerRef} className="relative">
      <input
        ref={inputRef}
        type="text"
        value={isOpen ? searchQuery : selectedDisplayText}
        onChange={(e) => {
          setSearchQuery(e.target.value);
          setHighlightedIndex(-1);
        }}
        onFocus={() => {
          setSearchQuery("");
          setIsOpen(true);
          setHighlightedIndex(-1);
        }}
        onKeyDown={handleKeyDown}
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
                selectItem("");
              }}
              onMouseEnter={() => setHighlightedIndex(0)}
              className={`px-3 py-2 text-sm cursor-pointer transition-colors ${
                highlightedIndex === 0
                  ? "bg-brand-primary/10 text-text-primary"
                  : "text-text-primary hover:bg-bg-page"
              } ${!selectedProjectId ? "font-medium" : ""}`}
            >
              {allOption.label}
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
                    selectItem(project.id);
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
