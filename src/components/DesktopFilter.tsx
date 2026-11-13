import { useEffect, useId, useRef, useState } from "react";
import { useTranslations } from "next-intl";

interface DesktopFilterProps {
  hosts: string[];
  selectedDesktops: string[];
  onChange: (desktops: string[]) => void;
}

export default function DesktopFilter({ hosts, selectedDesktops, onChange }: DesktopFilterProps) {
  const t = useTranslations("board");
  const tc = useTranslations("common");
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const options = [{ value: "local", label: tc("local") }, ...hosts.map((host) => ({ value: `ssh:${host}`, label: host }))];
  const selected = options.filter((option) => selectedDesktops.includes(option.value));
  const label = selected.length === 0 ? t("allDesktops")
    : selected.length === 1 ? selected[0].label : t("selectedDesktops", { count: selected.length });
  const toggleDesktop = (desktop: string) => onChange(selectedDesktops.includes(desktop)
    ? selectedDesktops.filter((value) => value !== desktop) : [...selectedDesktops, desktop]);

  useEffect(() => {
    if (!isOpen) return;
    const closeOutside = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) setIsOpen(false);
    };
    document.addEventListener("mousedown", closeOutside);
    containerRef.current?.querySelector<HTMLInputElement>("input")?.focus();
    return () => document.removeEventListener("mousedown", closeOutside);
  }, [isOpen]);

  return (
    <div
      ref={containerRef}
      className="relative w-48 shrink-0"
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setIsOpen(false);
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          setIsOpen(false);
          buttonRef.current?.focus();
        }
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        aria-label={t("desktopFilter")}
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        aria-controls={isOpen ? menuId : undefined}
        onClick={() => setIsOpen((open) => !open)}
        className="flex h-[34px] w-full items-center gap-2 rounded-md border border-border-default bg-bg-page px-3 text-sm text-text-primary hover:border-border-strong focus-visible:outline focus-visible:outline-2 focus-visible:outline-border-strong"
        data-testid="desktop-filter"
      >
        <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="shrink-0 text-text-muted">
          <rect x="3" y="4" width="18" height="13" rx="2" /><path d="M8 21h8M12 17v4" />
        </svg>
        <span className="truncate">{label}</span>
        <svg aria-hidden="true" width="12" height="12" viewBox="0 0 12 12" fill="none" className="ml-auto shrink-0 text-text-muted">
          <path d="m3 5 3 3 3-3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>
      {isOpen && (
        <div id={menuId} role="dialog" aria-label={t("desktopFilter")} className="absolute right-0 z-50 mt-1 max-h-64 w-56 overflow-y-auto rounded-md border border-border-default bg-bg-surface p-1 shadow-md">
          {[{ value: "", label: t("allDesktops") }, ...options].map((option) => (
            <label key={option.value} className="flex min-h-9 cursor-pointer items-center gap-3 rounded px-2 py-1.5 text-sm text-text-primary hover:bg-bg-page">
              <input
                type="checkbox"
                checked={option.value === "" ? selected.length === 0 : selectedDesktops.includes(option.value)}
                onChange={() => option.value === "" ? onChange([]) : toggleDesktop(option.value)}
                style={{ accentColor: "var(--color-text-primary)" }}
                className="h-3.5 w-3.5 shrink-0"
              />
              <span className="truncate">{option.label}</span>
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
