"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import HighlightedText from "@/components/HighlightedText";
import {
  getSearchableTasks,
  type SearchableTask,
} from "@/desktop/renderer/actions/kanban";
import { getTaskSearchShortcut } from "@/desktop/renderer/actions/appSettings";
import { localizeHref, usePathname, useRouter } from "@/desktop/renderer/navigation";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { openInternalRouteInNewWindow } from "@/desktop/renderer/utils/windowOpen";
import {
  DEFAULT_TASK_SEARCH_SHORTCUT,
  formatShortcutForDisplay,
  getCurrentShortcutPlatform,
  isBlockedShortcutEvent,
  matchShortcutEvent,
} from "@/desktop/renderer/utils/keyboardShortcut";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import { fuzzyMatch, type FuzzyMatch } from "@/utils/fuzzySearch";
import { CREATE_BRANCH_TODO_SHORTCUT, useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";

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

interface WeightedFieldMatch {
  key: SearchFieldKey;
  weight: number;
  match: FuzzyMatch;
}

interface TokenMatchSet {
  matches: WeightedFieldMatch[];
  score: number;
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

function findFieldMatches(token: string, fields: SearchFieldCandidate[]): WeightedFieldMatch[] {
  return fields
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
    .filter((match): match is WeightedFieldMatch => match !== null);
}

function scoreFieldMatch(match: WeightedFieldMatch) {
  return match.match.score + match.weight;
}

function findTokenMatchSet(token: string, fields: SearchFieldCandidate[]): TokenMatchSet | null {
  const directMatches = findFieldMatches(token, fields);

  if (directMatches.length > 0) {
    return {
      matches: directMatches,
      score: Math.max(...directMatches.map(scoreFieldMatch)),
    };
  }

  let bestSplitMatchSet: TokenMatchSet | null = null;

  for (let splitIndex = 1; splitIndex < token.length; splitIndex++) {
    const leftMatches = findFieldMatches(token.slice(0, splitIndex), fields);
    const rightMatches = findFieldMatches(token.slice(splitIndex), fields);

    for (const leftMatch of leftMatches) {
      for (const rightMatch of rightMatches) {
        if (leftMatch.key === rightMatch.key) {
          continue;
        }

        const splitScore = scoreFieldMatch(leftMatch) + scoreFieldMatch(rightMatch);
        if (!bestSplitMatchSet || splitScore > bestSplitMatchSet.score) {
          bestSplitMatchSet = {
            matches: [leftMatch, rightMatch],
            score: splitScore,
          };
        }
      }
    }
  }

  return bestSplitMatchSet;
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
        const tokenMatchSet = findTokenMatchSet(token, fields);

        if (!tokenMatchSet) {
          return null;
        }

        for (const tokenMatch of tokenMatchSet.matches) {
          fieldMatches[tokenMatch.key].push(tokenMatch.match);
        }

        score += tokenMatchSet.score;
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
  const boardCommands = useBoardCommands();
  const t = useTranslations("taskSearch");
  const tc = useTranslations("common");
  const router = useRouter();
  const pathname = usePathname();
  const refreshSignal = useRefreshSignal(["all", "settings"]);
  const inputRef = useRef<HTMLInputElement>(null);
  const resultsListRef = useRef<HTMLDivElement>(null);
  const [savedShortcut, setSavedShortcut] = useState<string>(DEFAULT_TASK_SEARCH_SHORTCUT);
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [tasks, setTasks] = useState<SearchableTask[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isLoading, setIsLoading] = useState(false);
  const shortcutPlatform = getCurrentShortcutPlatform();

  const effectiveShortcut = shortcut || savedShortcut;
  const results = useMemo(() => buildSearchResults(tasks, query), [query, tasks]);
  const selectedResultIndex = results.length === 0
    ? 0
    : Math.min(selectedIndex, results.length - 1);

  const openDialog = useCallback(() => {
    setQuery("");
    setSelectedIndex(0);
    setIsLoading(true);
    setIsOpen(true);
  }, []);

  const closeDialog = useCallback(() => {
    setIsOpen(false);
    setQuery("");
    setSelectedIndex(0);
    setIsLoading(false);
    requestActiveTerminalFocusAfterUiSettles();
  }, []);

  useEffect(() => {
    if (shortcut) {
      return;
    }

    let cancelled = false;

    getTaskSearchShortcut().then((nextShortcut) => {
      if (!cancelled) {
        setSavedShortcut(nextShortcut || DEFAULT_TASK_SEARCH_SHORTCUT);
      }
    });

    return () => {
      cancelled = true;
    };
  }, [refreshSignal, shortcut]);

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      if (isBlockedShortcutEvent(event, shortcutPlatform)) {
        event.preventDefault();
        return;
      }

      const eventTarget = event.target;
      if (eventTarget instanceof Element && eventTarget.closest('[data-shortcut-capture="true"]')) {
        return;
      }

      if (!matchShortcutEvent(event, effectiveShortcut, shortcutPlatform)) {
        return;
      }

      event.preventDefault();
      if (isOpen) {
        closeDialog();
        return;
      }

      openDialog();
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  }, [closeDialog, effectiveShortcut, isOpen, openDialog, shortcutPlatform]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    inputRef.current?.focus();

    let cancelled = false;

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
    boardCommands.setTaskQuickSearchOpen(isOpen);

    return () => {
      boardCommands.setTaskQuickSearchOpen(false);
    };
  }, [boardCommands, isOpen]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const selectedResult = resultsListRef.current?.children[selectedResultIndex];
    if (selectedResult instanceof HTMLElement) {
      selectedResult.scrollIntoView({ block: "nearest" });
    }
  }, [isOpen, results, selectedResultIndex]);

  useEscapeKey(closeDialog, { enabled: isOpen });

  function moveToTask(taskId: string) {
    router.push(`/task/${taskId}`);
    closeDialog();
  }

  function openTaskInNewWindow(taskId: string) {
    const currentLocale = pathname.split("/").filter(Boolean)[0];
    const taskHref = localizeHref(`/task/${taskId}`, currentLocale);
    openInternalRouteInNewWindow(taskHref);
    closeDialog();
  }

  const createBranchTodoFromSelection = useCallback(() => {
    const selectedTask = results[selectedResultIndex]?.task;

    if (!boardCommands.canCreateBranchTodo || !selectedTask?.projectId || !selectedTask.branchName) {
      return;
    }

    boardCommands.requestCreateBranchTodo({
      projectId: selectedTask.projectId,
      baseBranch: selectedTask.branchName,
    });
    closeDialog();
  }, [boardCommands, closeDialog, results, selectedResultIndex]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    return window.kanvibeDesktop?.onCreateTaskShortcut?.(() => {
      createBranchTodoFromSelection();
    });
  }, [createBranchTodoFromSelection, isOpen]);

  function handleInputKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    if (matchShortcutEvent(event, CREATE_BRANCH_TODO_SHORTCUT, shortcutPlatform)) {
      event.preventDefault();
      createBranchTodoFromSelection();
      return;
    }

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
        if (results[selectedResultIndex]) {
          const selectedTaskId = results[selectedResultIndex].task.id;
          if (event.shiftKey) {
            openTaskInNewWindow(selectedTaskId);
            break;
          }

          moveToTask(selectedTaskId);
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

  const footerHints = [
    t("hint"),
    boardCommands.canCreateBranchTodo
      ? t("branchTodoHint", {
          shortcut: formatShortcutForDisplay(CREATE_BRANCH_TODO_SHORTCUT, shortcutPlatform),
        })
      : null,
  ].filter(Boolean).join(" · ");

  return (
    <div data-terminal-focus-blocker="true" className="fixed inset-0 z-[500] flex items-start justify-center bg-black/45 px-4 pt-24">
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
            </div>
          </div>
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setSelectedIndex(0);
            }}
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
            <div ref={resultsListRef}>
              {results.map((result, index) => {
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
                      index === selectedResultIndex
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
              })}
            </div>
          )}
        </div>

        <div className="border-t border-border-default px-4 py-2 text-xs text-text-muted">
          {footerHints}
        </div>
      </div>
    </div>
  );
}
