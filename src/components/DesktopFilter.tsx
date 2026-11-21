import { forwardRef, useEffect, useId, useImperativeHandle, useRef, useState } from "react";
import { useTranslations } from "next-intl";

interface DesktopFilterProps {
  hosts: string[];
  selectedDesktops: string[];
  onChange: (desktops: string[]) => void;
}

export interface DesktopFilterHandle {
  open: () => void;
}

/** 프로젝트·태스크가 속한 데스크톱을 필터 선택값과 같은 형식으로 나타낸다 */
function getDesktopKey(sshHost: string | null | undefined): string {
  return sshHost ? `ssh:${sshHost}` : "local";
}

/** 지금 보드에 없는 호스트를 고른 값은 빼서, 사라진 호스트 때문에 보드가 비지 않게 한다 */
export function pickAvailableDesktops(selectedDesktops: string[], hosts: string[]): string[] {
  const availableDesktops = new Set(["local", ...hosts.map(getDesktopKey)]);
  return selectedDesktops.filter((desktop) => availableDesktops.has(desktop));
}

/** 데스크톱 선택이 비어 있으면 전체를 뜻하므로 모든 대상이 통과한다 */
export function isOnDesktops(sshHost: string | null | undefined, activeDesktops: string[]): boolean {
  return activeDesktops.length === 0 || activeDesktops.includes(getDesktopKey(sshHost));
}

const DesktopFilter = forwardRef<DesktopFilterHandle, DesktopFilterProps>(function DesktopFilter(
  { hosts, selectedDesktops, onChange },
  ref,
) {
  const t = useTranslations("board");
  const tc = useTranslations("common");
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const options = [{ value: "local", label: tc("local") }, ...hosts.map((host) => ({ value: getDesktopKey(host), label: host }))];
  const selected = options.filter((option) => selectedDesktops.includes(option.value));
  const label = selected.length === 0 ? t("allDesktops")
    : selected.length === 1 ? selected[0].label : t("selectedDesktops", { count: selected.length });
  const toggleDesktop = (desktop: string) => onChange(selectedDesktops.includes(desktop)
    ? selectedDesktops.filter((value) => value !== desktop) : [...selectedDesktops, desktop]);

  useImperativeHandle(ref, () => ({ open: () => setIsOpen(true) }), []);

  useEffect(() => {
    if (!isOpen) return;
    const closeOutside = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) setIsOpen(false);
    };
    document.addEventListener("mousedown", closeOutside);
    containerRef.current?.querySelector<HTMLButtonElement>('[role="checkbox"]')?.focus();
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
          {[{ value: "", label: t("allDesktops") }, ...options].map((option) => {
            const isChecked = option.value === "" ? selected.length === 0 : selectedDesktops.includes(option.value);
            // 행 전체를 포커스 가능한 버튼으로 두어야 이름을 눌러도 포커스가 메뉴 안에 남아 메뉴가 닫히지 않는다
            return (
              <button
                key={option.value}
                type="button"
                role="checkbox"
                aria-checked={isChecked}
                onClick={() => option.value === "" ? onChange([]) : toggleDesktop(option.value)}
                className="flex min-h-9 w-full cursor-pointer items-center gap-3 rounded px-2 py-1.5 text-left text-sm text-text-primary hover:bg-bg-page focus-visible:outline focus-visible:outline-2 focus-visible:outline-border-strong"
              >
                <span aria-hidden="true" className="flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-sm border border-border-strong">
                  {isChecked && (
                    <svg width="10" height="8" viewBox="0 0 10 8" fill="none">
                      <path d="M1 4L3.5 6.5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                    </svg>
                  )}
                </span>
                <span className="truncate">{option.label}</span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
});

export default DesktopFilter;
