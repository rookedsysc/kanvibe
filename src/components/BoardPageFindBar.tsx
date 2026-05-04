"use client";

import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useTranslations } from "next-intl";
import { SHORTCUTS, getCurrentShortcutPlatform, matchShortcutEvent } from "@/desktop/renderer/utils/keyboardShortcut";

const BOARD_PAGE_FIND_SHORTCUT = SHORTCUTS.boardPageFind;

function findPageText(query: string, backwards = false) {
  const trimmedQuery = query.trim();
  if (!trimmedQuery || typeof window.find !== "function") {
    return null;
  }

  return window.find(trimmedQuery, false, backwards, true, false, false, false);
}

export default function BoardPageFindBar() {
  const t = useTranslations("board");
  const tc = useTranslations("common");
  const inputRef = useRef<HTMLInputElement>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [hasMatch, setHasMatch] = useState<boolean | null>(null);
  const shortcutPlatform = getCurrentShortcutPlatform();

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });
  }, [isOpen]);

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      if (!matchShortcutEvent(event, BOARD_PAGE_FIND_SHORTCUT, shortcutPlatform)) {
        return;
      }

      event.preventDefault();
      setIsOpen(true);
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  }, [shortcutPlatform]);

  function closeSearchBar() {
    setIsOpen(false);
    setQuery("");
    setHasMatch(null);
  }

  function runSearch(backwards = false) {
    const searchResult = findPageText(query, backwards);
    if (searchResult === null) {
      return;
    }

    setHasMatch(searchResult);
  }

  function handleInputKeyDown(event: ReactKeyboardEvent<HTMLInputElement>) {
    switch (event.key) {
      case "Enter":
        event.preventDefault();
        runSearch(event.shiftKey);
        break;
      case "Escape":
        event.preventDefault();
        closeSearchBar();
        break;
    }
  }

  if (!isOpen) {
    return null;
  }

  return (
    <div className="fixed right-6 top-6 z-[450] w-full max-w-md rounded-xl border border-border-default bg-bg-surface p-3 shadow-xl">
      <div className="flex items-center gap-2">
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setHasMatch(null);
          }}
          onKeyDown={handleInputKeyDown}
          placeholder={t("pageFind.placeholder")}
          aria-label={t("pageFind.label")}
          className="min-w-0 flex-1 rounded-md border border-border-default bg-bg-page px-3 py-2 text-sm text-text-primary outline-none transition-colors focus:border-brand-primary"
        />
        <button
          type="button"
          onClick={() => runSearch(true)}
          className="rounded-md border border-border-default bg-bg-page px-2.5 py-2 text-xs text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
        >
          {t("pageFind.previous")}
        </button>
        <button
          type="button"
          onClick={() => runSearch(false)}
          className="rounded-md border border-border-default bg-bg-page px-2.5 py-2 text-xs text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
        >
          {t("pageFind.next")}
        </button>
        <button
          type="button"
          onClick={closeSearchBar}
          aria-label={tc("close")}
          className="rounded-md border border-border-default bg-bg-page px-2.5 py-2 text-xs text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
        >
          {tc("close")}
        </button>
      </div>
      {hasMatch === false ? (
        <p className="mt-2 text-xs text-text-muted">{t("pageFind.noMatch")}</p>
      ) : null}
    </div>
  );
}
