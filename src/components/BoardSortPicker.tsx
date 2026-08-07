"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import {
  BOARD_SORT_FIELDS,
  BOARD_SORT_MODES,
  type BoardSortField,
  type BoardSortMode,
  type BoardSortPreference,
} from "@/desktop/shared/boardSort";

interface BoardSortPickerProps {
  preference: BoardSortPreference;
  onChange: (preference: BoardSortPreference) => void;
}

function SortIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 6h16M6 12h12M9 18h6" />
    </svg>
  );
}

function DirectionIcon({ direction }: { direction: "asc" | "desc" }) {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {direction === "asc"
        ? <path d="M12 19V5m-5 5 5-5 5 5" />
        : <path d="M12 5v14m-5-5 5 5 5-5" />}
    </svg>
  );
}

function RemoveIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" aria-hidden="true">
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  );
}

export default function BoardSortPicker({ preference, onChange }: BoardSortPickerProps) {
  const t = useTranslations("board.sort");
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isOpen) return;

    function handleClickOutside(event: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [isOpen]);

  const findKeyIndex = useCallback(
    (field: BoardSortField) => preference.keys.findIndex((key) => key.field === field),
    [preference.keys],
  );

  /** 고른 기준을 켜고 끈다. 새로 고른 기준은 맨 뒤에 쌓여 앞선 기준이 같을 때만 개입한다 */
  function toggleField(field: BoardSortField) {
    const index = findKeyIndex(field);
    const keys = index >= 0
      ? preference.keys.filter((key) => key.field !== field)
      : [...preference.keys, { field, direction: "asc" as const }];

    onChange({ ...preference, keys });
  }

  function toggleDirection(field: BoardSortField) {
    onChange({
      ...preference,
      keys: preference.keys.map((key) => (
        key.field === field
          ? { ...key, direction: key.direction === "asc" ? "desc" : "asc" }
          : key
      )),
    });
  }

  function changeMode(mode: BoardSortMode) {
    onChange({ ...preference, mode });
  }

  const activeCount = preference.keys.length;
  const summary = preference.keys
    .map((key) => `${t(`fields.${key.field}`)} ${t(`directions.${key.field}.${key.direction}`)}`)
    .join(" · ");

  return (
    <div className="relative shrink-0" ref={containerRef} data-testid="board-sort-control">
      <button
        type="button"
        onClick={() => setIsOpen((previous) => !previous)}
        aria-expanded={isOpen}
        aria-label={activeCount === 0 ? t("label") : `${t("label")}, ${summary}`}
        title={activeCount === 0 ? t("label") : summary}
        className={`flex h-[34px] items-center gap-1.5 rounded-md border px-2.5 text-sm font-medium transition-colors ${
          activeCount > 0
            ? "border-border-brand bg-brand-subtle text-text-primary"
            : "border-border-default bg-bg-surface text-text-secondary hover:border-brand-primary hover:text-text-primary"
        }`}
      >
        <SortIcon />
        {t("label")}
        {activeCount > 0 && (
          <span
            data-testid="board-sort-active-count"
            className="rounded-full bg-brand-primary px-1.5 text-[11px] font-semibold text-text-inverse"
          >
            {activeCount}
          </span>
        )}
      </button>

      {isOpen && (
        <div
          role="dialog"
          aria-label={t("label")}
          className="absolute right-0 z-50 mt-1 w-72 rounded-lg border border-border-default bg-bg-surface p-3 shadow-lg"
        >
          {activeCount === 0 ? (
            <p className="mb-2 text-xs text-text-muted">{t("hint")}</p>
          ) : (
            <div className="mb-2 flex flex-col gap-1">
              {preference.keys.map((key, index) => (
                <div
                  key={key.field}
                  className="flex items-center gap-2 rounded-md border border-border-subtle bg-bg-page px-2 py-1.5"
                >
                  <span aria-hidden="true" className="text-[11px] font-semibold text-text-muted">
                    {index + 1}
                  </span>
                  <span className="flex-1 text-sm text-text-primary">{t(`fields.${key.field}`)}</span>
                  <button
                    type="button"
                    onClick={() => toggleDirection(key.field)}
                    aria-label={t("toggleDirection", {
                      field: t(`fields.${key.field}`),
                      direction: t(`directions.${key.field}.${key.direction}`),
                    })}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-text-secondary transition-colors hover:bg-bg-surface hover:text-text-primary"
                  >
                    <DirectionIcon direction={key.direction} />
                    {t(`directions.${key.field}.${key.direction}`)}
                  </button>
                  <button
                    type="button"
                    onClick={() => toggleField(key.field)}
                    aria-label={t("removeField", { field: t(`fields.${key.field}`) })}
                    className="rounded p-0.5 text-text-muted transition-colors hover:bg-bg-surface hover:text-text-primary"
                  >
                    <RemoveIcon />
                  </button>
                </div>
              ))}
            </div>
          )}

          <div role="listbox" aria-multiselectable aria-label={t("fieldListLabel")} className="flex flex-col">
            {BOARD_SORT_FIELDS.map((field) => {
              const isActive = findKeyIndex(field) >= 0;
              return (
                <button
                  key={field}
                  type="button"
                  role="option"
                  aria-selected={isActive}
                  onClick={() => toggleField(field)}
                  className={`flex items-center gap-2 rounded px-2 py-1.5 text-left text-sm transition-colors hover:bg-bg-page ${
                    isActive ? "text-text-primary" : "text-text-secondary"
                  }`}
                >
                  <span
                    aria-hidden="true"
                    className={`flex h-4 w-4 items-center justify-center rounded border text-[10px] ${
                      isActive
                        ? "border-brand-primary bg-brand-primary text-text-inverse"
                        : "border-border-default"
                    }`}
                  >
                    {isActive ? "✓" : ""}
                  </span>
                  {t(`fields.${field}`)}
                </button>
              );
            })}
          </div>

          <div className="mt-3 border-t border-border-subtle pt-2">
            <p className="mb-1.5 text-[11px] font-semibold uppercase text-text-muted">{t("modeLabel")}</p>
            <div role="radiogroup" aria-label={t("modeLabel")} className="flex flex-col gap-0.5">
              {BOARD_SORT_MODES.map((mode) => (
                <button
                  key={mode}
                  type="button"
                  role="radio"
                  aria-checked={preference.mode === mode}
                  onClick={() => changeMode(mode)}
                  className={`rounded px-2 py-1.5 text-left text-sm transition-colors ${
                    preference.mode === mode
                      ? "bg-brand-subtle text-text-primary"
                      : "text-text-secondary hover:bg-bg-page"
                  }`}
                >
                  {t(`modes.${mode}`)}
                </button>
              ))}
            </div>
            <p data-testid="board-sort-mode-description" className="mt-1.5 text-xs leading-relaxed text-text-muted">
              {t(`modeDescriptions.${preference.mode}`)}
            </p>
          </div>

          <div className="mt-3 flex items-center justify-between border-t border-border-subtle pt-2">
            <button
              type="button"
              disabled={activeCount === 0}
              onClick={() => onChange({ ...preference, keys: [] })}
              className="text-xs text-text-muted transition-colors hover:text-text-primary disabled:opacity-40"
            >
              {t("clear")}
            </button>
            <button
              type="button"
              onClick={() => setIsOpen(false)}
              className="text-xs font-medium text-brand-primary transition-colors hover:text-brand-hover"
            >
              {t("done")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
