"use client";

import { useState, useEffect, useTransition, useRef, useCallback } from "react";
import { useTranslations } from "next-intl";
import { createTask } from "@/app/actions/kanban";
import { getProjectBranches } from "@/app/actions/project";
import { SessionType } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";

interface CreateTaskModalProps {
  isOpen: boolean;
  onClose: () => void;
  sshHosts: string[];
  projects: Project[];
  defaultProjectId?: string;
}

export default function CreateTaskModal({
  isOpen,
  onClose,
  sshHosts,
  projects,
  defaultProjectId,
}: CreateTaskModalProps) {
  const t = useTranslations("task");
  const tc = useTranslations("common");
  const [isPending, startTransition] = useTransition();
  const [selectedProjectId, setSelectedProjectId] = useState(defaultProjectId || "");
  const [branches, setBranches] = useState<string[]>([]);
  const [baseBranch, setBaseBranch] = useState("");

  /** 프로젝트 검색 관련 state */
  const [searchQuery, setSearchQuery] = useState("");
  const [isDropdownOpen, setIsDropdownOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  /** 모달이 열릴 때 필터에서 선택된 프로젝트를 자동 설정한다 */
  useEffect(() => {
    if (isOpen && defaultProjectId) {
      setSelectedProjectId(defaultProjectId);
    }
  }, [isOpen, defaultProjectId]);

  /** worktree가 아닌 프로젝트만 필터링한다 */
  const availableProjects = projects.filter((p) => !p.isWorktree);

  /** 검색어가 있으면 추가 필터링, 없으면 전체 목록을 표시한다 */
  const filteredProjects = searchQuery
    ? availableProjects.filter((p) =>
        p.name.toLowerCase().includes(searchQuery.toLowerCase())
      )
    : availableProjects;

  useEffect(() => {
    if (!selectedProjectId) {
      setBranches([]);
      setBaseBranch("");
      return;
    }

    const selectedProject = projects.find((p) => p.id === selectedProjectId);
    if (selectedProject) {
      setBaseBranch(selectedProject.defaultBranch);
    }

    getProjectBranches(selectedProjectId).then((result) => {
      setBranches(result);
    });
  }, [selectedProjectId, projects]);

  /** 드롭다운 외부 클릭 시 닫고 검색어를 초기화한다 */
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setIsDropdownOpen(false);
        setSearchQuery("");
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  /** 선택된 프로젝트의 표시 텍스트를 반환한다 */
  const selectedDisplayText = (() => {
    const selected = projects.find((p) => p.id === selectedProjectId);
    if (!selected) return "";
    return selected.name + (selected.sshHost ? ` (${selected.sshHost})` : "");
  })();

  const selectProject = useCallback((project: Project) => {
    setSelectedProjectId(project.id);
    setSearchQuery("");
    setIsDropdownOpen(false);
    setHighlightedIndex(-1);
  }, []);

  /** 키보드 네비게이션 핸들러 */
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (!isDropdownOpen) {
        if (e.key === "ArrowDown" || e.key === "Enter") {
          e.preventDefault();
          setIsDropdownOpen(true);
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
          if (highlightedIndex >= 0 && highlightedIndex < filteredProjects.length) {
            selectProject(filteredProjects[highlightedIndex]);
          }
          break;
        case "Escape":
          e.preventDefault();
          setIsDropdownOpen(false);
          setSearchQuery("");
          setHighlightedIndex(-1);
          inputRef.current?.blur();
          break;
      }
    },
    [isDropdownOpen, highlightedIndex, filteredProjects, selectProject]
  );

  if (!isOpen) return null;

  function handleSubmit(formData: FormData) {
    const branchName = formData.get("branchName") as string;
    if (!branchName || !selectedProjectId) return;

    startTransition(async () => {
      await createTask({
        title: branchName,
        description: (formData.get("description") as string) || undefined,
        branchName,
        baseBranch: baseBranch || undefined,
        sessionType: (formData.get("sessionType") as SessionType) || undefined,
        sshHost: (formData.get("sshHost") as string) || undefined,
        projectId: selectedProjectId,
      });
      onClose();
    });
  }

  return (
    <div className="fixed inset-0 z-[400] flex items-center justify-center bg-bg-overlay">
      <div className="w-full max-w-md bg-bg-surface rounded-xl border border-border-default shadow-lg p-6">
        <h2 className="text-lg font-semibold text-text-primary mb-4">
          {t("createTitle")}
        </h2>

        <form action={handleSubmit} className="space-y-4">
          <div ref={dropdownRef} className="relative">
            <label className="block text-sm text-text-secondary mb-1">
              {t("project")} *
            </label>
            <input
              ref={inputRef}
              type="text"
              value={isDropdownOpen ? searchQuery : selectedDisplayText}
              onChange={(e) => {
                setSearchQuery(e.target.value);
                setHighlightedIndex(-1);
                if (!e.target.value) {
                  setSelectedProjectId("");
                }
              }}
              onFocus={() => {
                setSearchQuery("");
                setIsDropdownOpen(true);
                setHighlightedIndex(-1);
              }}
              onKeyDown={handleKeyDown}
              placeholder={isDropdownOpen ? t("projectSearch") : t("projectSelect")}
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors cursor-pointer"
            />
            {isDropdownOpen && (
              <ul className="absolute z-50 mt-1 w-full max-h-48 overflow-y-auto bg-bg-surface border border-border-default rounded-md shadow-md">
                {filteredProjects.length === 0 ? (
                  <li className="px-3 py-2 text-sm text-text-muted">
                    {t("projectSelect")}
                  </li>
                ) : (
                  filteredProjects.map((project, index) => (
                    <li
                      key={project.id}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        selectProject(project);
                      }}
                      onMouseEnter={() => setHighlightedIndex(index)}
                      className={`px-3 py-2 text-sm cursor-pointer transition-colors ${
                        index === highlightedIndex
                          ? "bg-brand-primary/10 text-text-primary"
                          : "text-text-primary hover:bg-bg-page"
                      } ${
                        project.id === selectedProjectId
                          ? "font-medium"
                          : ""
                      }`}
                    >
                      {project.name}
                      {project.sshHost && (
                        <span className="text-text-muted ml-1">
                          ({project.sshHost})
                        </span>
                      )}
                    </li>
                  ))
                )}
              </ul>
            )}
          </div>

          {selectedProjectId && (
            <div>
              <label className="block text-sm text-text-secondary mb-1">
                {t("baseBranch")}
              </label>
              <select
                value={baseBranch}
                onChange={(e) => setBaseBranch(e.target.value)}
                className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
              >
                {branches.map((branch) => (
                  <option key={branch} value={branch}>
                    {branch}
                  </option>
                ))}
                {branches.length === 0 && baseBranch && (
                  <option value={baseBranch}>{baseBranch}</option>
                )}
              </select>
            </div>
          )}

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("branchName")} *
            </label>
            <input
              name="branchName"
              required
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
              placeholder={t("branchPlaceholder")}
            />
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("descriptionLabel")}
            </label>
            <textarea
              name="description"
              rows={3}
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary resize-none transition-colors"
              placeholder={t("descriptionPlaceholder")}
            />
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("sessionType")}
            </label>
            <select
              name="sessionType"
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
            >
              <option value="tmux">tmux</option>
              <option value="zellij">zellij</option>
            </select>
          </div>

          <div className="flex justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm text-text-muted hover:text-text-primary transition-colors"
            >
              {tc("cancel")}
            </button>
            <button
              type="submit"
              disabled={isPending || !selectedProjectId}
              className="px-4 py-2 text-sm bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md font-medium transition-colors"
            >
              {isPending ? tc("creating") : tc("create")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
