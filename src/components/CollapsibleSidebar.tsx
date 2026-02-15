"use client";

import { useState, useTransition, useRef, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { useTranslations } from "next-intl";
import { dismissSidebarHint } from "@/app/actions/appSettings";

interface CollapsibleSidebarProps {
  defaultCollapsed: boolean;
  showHint: boolean;
  children: React.ReactNode;
}

/** 사이드바 패널 토글 아이콘 */
function SidebarPanelIcon({ className }: { className?: string }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.2"
      className={className}
    >
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <line x1="6" y1="2.5" x2="6" y2="13.5" />
    </svg>
  );
}

/** 포탈로 body에 렌더링되는 힌트 말풍선. 버튼 위에 fixed 배치. */
function ToggleButtonWithHint({
  onClick,
  buttonClassName,
  showHint,
  onDismissHint,
  children,
}: {
  onClick: () => void;
  buttonClassName: string;
  showHint: boolean;
  onDismissHint: () => void;
  children: React.ReactNode;
}) {
  const t = useTranslations("taskDetail");
  const [isPending, startTransition] = useTransition();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [hintPos, setHintPos] = useState<{ top: number; left: number } | null>(null);
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  const updateHintPosition = useCallback(() => {
    if (!buttonRef.current) return;
    const rect = buttonRef.current.getBoundingClientRect();
    setHintPos({
      top: rect.top,
      left: rect.left + rect.width / 2,
    });
  }, []);

  useEffect(() => {
    if (!showHint || !mounted) return;
    updateHintPosition();
    window.addEventListener("resize", updateHintPosition);
    window.addEventListener("scroll", updateHintPosition, true);
    return () => {
      window.removeEventListener("resize", updateHintPosition);
      window.removeEventListener("scroll", updateHintPosition, true);
    };
  }, [showHint, mounted, updateHintPosition]);

  function handleDismiss() {
    startTransition(async () => {
      await dismissSidebarHint();
      onDismissHint();
    });
  }

  return (
    <>
      <button ref={buttonRef} onClick={onClick} className={buttonClassName}>
        {children}
      </button>
      {mounted && showHint && hintPos && createPortal(
        <div
          style={{ top: hintPos.top, left: hintPos.left }}
          className="fixed -translate-x-1/2 -translate-y-full z-[9999] pb-2"
        >
          <div className="bg-[#333] text-white text-xs rounded-lg px-3 py-2.5 shadow-lg whitespace-nowrap">
            <div>{t("sidebarTooltip")}</div>
            <button
              onClick={handleDismiss}
              disabled={isPending}
              className="mt-1.5 text-xs opacity-60 hover:opacity-100 underline underline-offset-2 transition-opacity"
            >
              {t("dismissHint")}
            </button>
          </div>
          <div className="flex justify-center">
            <div className="border-[5px] border-transparent border-t-[#333]" />
          </div>
        </div>,
        document.body
      )}
    </>
  );
}

export default function CollapsibleSidebar({
  defaultCollapsed,
  showHint,
  children,
}: CollapsibleSidebarProps) {
  const [isCollapsed, setIsCollapsed] = useState(defaultCollapsed);
  const [isHintVisible, setIsHintVisible] = useState(showHint);
  const t = useTranslations("taskDetail");

  if (isCollapsed) {
    return (
      <div className="shrink-0 flex flex-col h-full">
        <div className="flex-1" />
        <div className="border-t border-border-default pt-3">
          <ToggleButtonWithHint
            onClick={() => setIsCollapsed(false)}
            buttonClassName="p-2 bg-bg-surface border border-border-default rounded-lg hover:border-brand-primary text-text-muted hover:text-text-primary transition-colors"
            showHint={isHintVisible}
            onDismissHint={() => setIsHintVisible(false)}
          >
            <SidebarPanelIcon />
          </ToggleButtonWithHint>
        </div>
      </div>
    );
  }

  return (
    <aside className="lg:w-80 shrink-0 flex flex-col lg:overflow-y-auto">
      <div className="flex flex-col gap-4 flex-1">
        {children}
      </div>
      <div className="border-t border-border-default mt-4 pt-3 flex justify-end">
        <ToggleButtonWithHint
          onClick={() => setIsCollapsed(true)}
          buttonClassName="p-1.5 text-text-muted hover:text-text-primary transition-colors"
          showHint={isHintVisible}
          onDismissHint={() => setIsHintVisible(false)}
        >
          <SidebarPanelIcon />
        </ToggleButtonWithHint>
      </div>
    </aside>
  );
}
