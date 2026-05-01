"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import HighlightedText from "@/components/HighlightedText";
import {
  getSearchableTasks,
  type SearchableTask,
} from "@/desktop/renderer/actions/kanban";
import { getTaskSearchShortcut } from "@/desktop/renderer/actions/appSettings";
import { useRouter } from "@/desktop/renderer/navigation";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import {
  DEFAULT_TASK_SEARCH_SHORTCUT,
  formatShortcutForDisplay,
  matchShortcutEvent,
} from "@/desktop/renderer/utils/keyboardShortcut";
import { fuzzyMatch, type FuzzyMatch } from "@/utils/fuzzySearch";

interface TaskQuickSearchDialogProps {
  shortcut?: string;
}

interface SearchResult {
  task: SearchableTask;
  score: number;
  branchMatch: FuzzyMatch | null;
  projectMatch: FuzzyMatch | null;
  titleMatch: FuzzyMatch | null;
}

type SearchFieldKey = "branch" | "project" | "title";

interface SearchFieldCandidate {
  key: SearchFieldKey;
  value: string | null | undefined;
  weight: number;
}

function isMacLikePlatform() {
  return typeof navigator !== "undefined"
    && (navigator.userAgent.includes("Mac") || navigator.platform.toLowerCase().includes("mac"));
}

function tokenizeQuery(query: string) {
  return query
    .trim()
    .split(/\s+/)
    .filter(Boolean);
}

function mergeMatchedIndices(matches: FuzzyMatch[]) {
  return [...new Set(matches.flatMap((match) => match.matchedIndices))].sort((left, right) => left - right);
}

function combineFieldMatches(
  value: string | null | undefined,
  matches: FuzzyMatch[],
): FuzzyMatch | null {
  if (!value || matches.length === 0) {
    return null;
  }

  return {
    path: value,
    score: matches.reduce((total, match) => total + match.score, 0),
    matchedIndices: mergeMatchedIndices(matches),
  };
}

function buildSearchResults(tasks: SearchableTask[], query: string): SearchResult[] {
  const trimmedQuery = query.trim();

  if (!trimmedQuery) {
    return tasks.map((task) => ({
      task,
      score: 0,
      branchMatch: null,
      projectMatch: null,
      titleMatch: null,
    }));
  }

  const tokens = tokenizeQuery(trimmedQuery);

  return tasks
    .map((task) => {
      const fields: SearchFieldCandidate[] = [
        { key: "branch", value: task.branchName, weight: 3 },
        { key: "project", value: task.projectName, weight: 2 },
        { key: "title", value: task.title, weight: 1 },
      ];
      const fieldMatches: Record<SearchFieldKey, FuzzyMatch[]> = {
        branch: [],
        project: [],
        title: [],
      };
      let score = 0;

      for (const token of tokens) {
        const tokenMatches = fields
          .map((field) => {
            if (!field.value) {
              return null;
            }

            const match = fuzzyMatch(token, field.value);
            if (!match) {
              return null;
            }

            return {
              key: field.key,
              weight: field.weight,
              match,
            };
          })
          .filter((match): match is { key: SearchFieldKey; weight: number; match: FuzzyMatch } => match !== null);

        if (tokenMatches.length === 0) {
          return null;
        }

        for (const tokenMatch of tokenMatches) {
          fieldMatches[tokenMatch.key].push(tokenMatch.match);
        }

        const bestTokenMatch = tokenMatches.reduce((best, current) => {
          const bestScore = best.match.score + best.weight;
          const currentScore = current.match.score + current.weight;
          return currentScore > bestScore ? current : best;
        });

        score += bestTokenMatch.match.score + bestTokenMatch.weight;
      }

      const branchMatch = combineFieldMatches(task.branchName, fieldMatches.branch);
      const projectMatch = combineFieldMatches(task.projectName, fieldMatches.project);
      const titleMatch = combineFieldMatches(task.title, fieldMatches.title);

      return {
        task,
        score,
        branchMatch,
        projectMatch,
        titleMatch,
      };
    })
    .filter((result): result is SearchResult => result !== null)
    .sort((left, right) => right.score - left.score);
}

export default function TaskQuickSearchDialog({
  shortcut,
}: TaskQuickSearchDialogProps) {
  const t = useTranslations("taskSearch");
  const tc = useTranslations("common");
  const router = useRouter();
  const refreshSignal = useRefreshSignal(["all", "settings"]);
  const inputRef = useRef<HTMLInputElement>(null);
  const [resolvedShortcut, setResolvedShortcut] = useState(shortcut || DEFAULT_TASK_SEARCH_SHORTCUT);
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [tasks, setTasks] = useState<SearchableTask[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isLoading, setIsLoading] = useState(false);
  const isMacLike = isMacLikePlatform();

  const effectiveShortcut = shortcut || resolvedShortcut;
  const results = useMemo(() => buildSearchResults(tasks, query), [query, tasks]);

  useEffect(() => {
    if (shortcut) {
      setResolvedShortcut(shortcut);
      return;
    }

    let cancelled = false;

    getTaskSearchShortcut().then((nextShortcut) => {
      if (!cancelled) {
        setResolvedShortcut(nextShortcut || DEFAULT_TASK_SEARCH_SHORTCUT);
      }
    });

    return () => {
      cancelled = true;
    };
  }, [refreshSignal, shortcut]);

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      const eventTarget = event.target;
      if (eventTarget instanceof Element && eventTarget.closest('[data-shortcut-capture="true"]')) {
        return;
      }

      if (!matchShortcutEvent(event, effectiveShortcut, isMacLike)) {
        return;
      }

      event.preventDefault();
      setIsOpen((current) => !current);
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  }, [effectiveShortcut, isMacLike]);

  useEffect(() => {
    if (!isOpen) {
      setQuery("");
      setSelectedIndex(0);
      return;
    }

    inputRef.current?.focus();

    let cancelled = false;
    setIsLoading(true);

    getSearchableTasks()
      .then((nextTasks) => {
        if (!cancelled) {
          setTasks(nextTasks);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [isOpen]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  useEffect(() => {
    if (selectedIndex >= results.length) {
      setSelectedIndex(0);
    }
  }, [results.length, selectedIndex]);

  function closeDialog() {
    setIsOpen(false);
  }

  function moveToTask(taskId: string) {
    router.push(`/task/${taskId}`);
    closeDialog();
  }

  function handleInputKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setSelectedIndex((current) => (results.length === 0 ? 0 : Math.min(current + 1, results.length - 1)));
        break;
      case "ArrowUp":
        event.preventDefault();
        setSelectedIndex((current) => Math.max(current - 1, 0));
        break;
      case "Enter":
        event.preventDefault();
        if (results[selectedIndex]) {
          moveToTask(results[selectedIndex].task.id);
        }
        break;
      case "Escape":
        event.preventDefault();
        closeDialog();
        break;
    }
  }

  if (!isOpen) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-[500] flex items-start justify-center bg-black/45 px-4 pt-24">
      <button
        type="button"
        aria-label={tc("close")}
        className="absolute inset-0 cursor-default"
        onClick={closeDialog}
      />
      <div
        role="dialog"
        aria-modal="true"
        className="relative z-10 w-full max-w-2xl overflow-hidden rounded-2xl border border-border-default bg-bg-surface shadow-2xl"
      >
        <div className="border-b border-border-default px-4 py-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-sm font-semibold text-text-primary">{t("title")}</p>
              <p className="text-xs text-text-muted">
                {formatShortcutForDisplay(effectiveShortcut, isMacLike)}
              </p>
            </div>
          </div>
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={handleInputKeyDown}
            placeholder={t("placeholder")}
            className="mt-3 w-full rounded-md border border-border-default bg-bg-page px-3 py-2 text-sm text-text-primary outline-none transition-colors focus:border-brand-primary"
          />
        </div>

        <div className="max-h-[420px] overflow-y-auto">
          {isLoading ? (
            <div className="px-4 py-6 text-sm text-text-muted">{tc("loading")}</div>
          ) : results.length === 0 ? (
            <div className="px-4 py-6 text-sm text-text-muted">{t("empty")}</div>
          ) : (
            results.map((result, index) => {
              const { task, branchMatch, projectMatch, titleMatch } = result;
              const isRemote = Boolean(task.sshHost);
              const primaryLabel = task.branchName || task.title;
              const primaryMatch = task.branchName ? branchMatch : titleMatch;

              return (
                <button
                  key={task.id}
                  type="button"
                  onClick={() => moveToTask(task.id)}
                  onMouseEnter={() => setSelectedIndex(index)}
                  className={`flex w-full items-start justify-between gap-4 border-b border-border-subtle px-4 py-3 text-left transition-colors ${
                    index === selectedIndex
                      ? "bg-brand-primary/10"
                      : "hover:bg-bg-page"
                  }`}
                >
                  <div className="min-w-0">
                    <div className="text-sm font-medium text-text-primary">
                      {primaryMatch ? (
                        <HighlightedText
                          text={primaryLabel}
                          matchedIndices={primaryMatch.matchedIndices}
                        />
                      ) : (
                        primaryLabel
                      )}
                    </div>
                    {task.branchName && task.title !== task.branchName ? (
                      <div className="mt-1 truncate text-xs text-text-muted">
                        {titleMatch ? (
                          <HighlightedText
                            text={task.title}
                            matchedIndices={titleMatch.matchedIndices}
                          />
                        ) : (
                          task.title
                        )}
                      </div>
                    ) : null}
                    {task.projectName ? (
                      <div className="mt-1 truncate text-xs text-text-muted">
                        {projectMatch ? (
                          <HighlightedText
                            text={task.projectName}
                            matchedIndices={projectMatch.matchedIndices}
                          />
                        ) : (
                          task.projectName
                        )}
                      </div>
                    ) : null}
                  </div>

                  <div className="flex shrink-0 items-center gap-2">
                    {isRemote && task.sshHost ? (
                      <span className="rounded-full bg-tag-gemini-bg px-2 py-0.5 text-[11px] font-medium text-tag-gemini-text">
                        {tc("remote")}
                      </span>
                    ) : null}
                    {task.sshHost ? (
                      <span className="text-[11px] text-text-muted">{task.sshHost}</span>
                    ) : null}
                  </div>
                </button>
              );
            })
          )}
        </div>

        <div className="border-t border-border-default px-4 py-2 text-xs text-text-muted">
          {t("hint")}
        </div>
      </div>
    </div>
  );
}
