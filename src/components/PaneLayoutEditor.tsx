"use client";

import { useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import {
  PaneLayoutType,
  type PaneCommand,
  type PaneLayoutConfig,
} from "@/entities/PaneLayoutConfig";
import { savePaneLayout, deletePaneLayout } from "@/app/actions/paneLayout";

interface PaneLayoutEditorProps {
  projectId?: string;
  initialConfig?: PaneLayoutConfig | null;
  isGlobal?: boolean;
}

/** 레이아웃 타입별 pane 위치 라벨 매핑 */
const PANE_LABELS: Record<PaneLayoutType, string[]> = {
  [PaneLayoutType.SINGLE]: ["paneLeft"],
  [PaneLayoutType.HORIZONTAL_2]: ["paneTop", "paneBottom"],
  [PaneLayoutType.VERTICAL_2]: ["paneLeft", "paneRight"],
  [PaneLayoutType.LEFT_RIGHT_TB]: ["paneLeft", "paneRightTop", "paneRightBottom"],
  [PaneLayoutType.LEFT_TB_RIGHT]: ["paneLeftTop", "paneLeftBottom", "paneRight"],
  [PaneLayoutType.QUAD]: ["paneTopLeft", "paneTopRight", "paneBottomLeft", "paneBottomRight"],
};

/** 레이아웃 타입별 번역 키 */
const LAYOUT_LABEL_KEYS: Record<PaneLayoutType, string> = {
  [PaneLayoutType.SINGLE]: "layoutSingle",
  [PaneLayoutType.HORIZONTAL_2]: "layoutHorizontal2",
  [PaneLayoutType.VERTICAL_2]: "layoutVertical2",
  [PaneLayoutType.LEFT_RIGHT_TB]: "layoutLeftRightTB",
  [PaneLayoutType.LEFT_TB_RIGHT]: "layoutLeftTBRight",
  [PaneLayoutType.QUAD]: "layoutQuad",
};

/** 레이아웃 미니 프리뷰 SVG */
function LayoutPreview({ type, isSelected }: { type: PaneLayoutType; isSelected: boolean }) {
  const borderColor = isSelected ? "var(--color-brand-primary)" : "var(--color-border-default)";
  const fillColor = isSelected ? "var(--color-brand-primary-light, #dbeafe)" : "var(--color-bg-page)";
  const strokeWidth = isSelected ? 2 : 1;

  const renders: Record<PaneLayoutType, React.ReactNode> = {
    [PaneLayoutType.SINGLE]: (
      <rect x="2" y="2" width="36" height="26" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
    ),
    [PaneLayoutType.HORIZONTAL_2]: (
      <>
        <rect x="2" y="2" width="36" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="2" y="16" width="36" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
      </>
    ),
    [PaneLayoutType.VERTICAL_2]: (
      <>
        <rect x="2" y="2" width="17" height="26" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="21" y="2" width="17" height="26" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
      </>
    ),
    [PaneLayoutType.LEFT_RIGHT_TB]: (
      <>
        <rect x="2" y="2" width="17" height="26" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="21" y="2" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="21" y="16" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
      </>
    ),
    [PaneLayoutType.LEFT_TB_RIGHT]: (
      <>
        <rect x="2" y="2" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="2" y="16" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="21" y="2" width="17" height="26" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
      </>
    ),
    [PaneLayoutType.QUAD]: (
      <>
        <rect x="2" y="2" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="21" y="2" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="2" y="16" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
        <rect x="21" y="16" width="17" height="12" rx="2" fill={fillColor} stroke={borderColor} strokeWidth={strokeWidth} />
      </>
    ),
  };

  return (
    <svg width="40" height="30" viewBox="0 0 40 30">
      {renders[type]}
    </svg>
  );
}

export default function PaneLayoutEditor({
  projectId,
  initialConfig,
  isGlobal = false,
}: PaneLayoutEditorProps) {
  const t = useTranslations("paneLayout");
  const tc = useTranslations("common");
  const [isPending, startTransition] = useTransition();
  const [message, setMessage] = useState<{ type: "success" | "error"; text: string } | null>(null);

  const [layoutType, setLayoutType] = useState<PaneLayoutType>(
    (initialConfig?.layoutType as PaneLayoutType) ?? PaneLayoutType.SINGLE
  );

  const paneLabels = PANE_LABELS[layoutType];
  const initialPanes = initialConfig?.panes ?? [];

  const [paneCommands, setPaneCommands] = useState<string[]>(
    paneLabels.map((_, i) => initialPanes[i]?.command ?? "")
  );

  /** 레이아웃 변경 시 pane 명령어 배열 크기 조정 */
  function handleLayoutChange(newType: PaneLayoutType) {
    setLayoutType(newType);
    const newLabels = PANE_LABELS[newType];
    setPaneCommands((prev) =>
      newLabels.map((_, i) => prev[i] ?? "")
    );
  }

  function handleCommandChange(index: number, value: string) {
    setPaneCommands((prev) => {
      const next = [...prev];
      next[index] = value;
      return next;
    });
  }

  function handleSave() {
    setMessage(null);
    const panes: PaneCommand[] = paneLabels.map((_, i) => ({
      position: i,
      command: paneCommands[i] ?? "",
    }));

    startTransition(async () => {
      try {
        await savePaneLayout({
          layoutType,
          panes,
          projectId: isGlobal ? null : projectId,
          isGlobal,
        });
        setMessage({ type: "success", text: t("saveSuccess") });
      } catch {
        setMessage({ type: "error", text: t("saveError") });
      }
    });
  }

  function handleDelete() {
    if (!initialConfig?.id) return;
    setMessage(null);

    startTransition(async () => {
      try {
        await deletePaneLayout(initialConfig.id);
        setLayoutType(PaneLayoutType.SINGLE);
        setPaneCommands([""]);
        setMessage({ type: "success", text: t("deleteSuccess") });
      } catch {
        setMessage({ type: "error", text: t("deleteError") });
      }
    });
  }

  return (
    <div className="space-y-4">
      {/* 레이아웃 타입 선택 */}
      <div>
        <label className="block text-xs text-text-muted uppercase tracking-wide mb-2">
          {t("layoutType")}
        </label>
        <div className="grid grid-cols-3 gap-2">
          {Object.values(PaneLayoutType).map((type) => (
            <button
              key={type}
              type="button"
              onClick={() => handleLayoutChange(type)}
              className={`flex flex-col items-center gap-1 p-2 rounded-lg border transition-colors ${
                layoutType === type
                  ? "border-brand-primary bg-brand-primary/5"
                  : "border-border-default hover:border-border-strong"
              }`}
            >
              <LayoutPreview type={type} isSelected={layoutType === type} />
              <span className="text-xs text-text-secondary">
                {t(LAYOUT_LABEL_KEYS[type])}
              </span>
            </button>
          ))}
        </div>
      </div>

      {/* 각 pane 명령어 입력 */}
      <div className="space-y-2">
        {paneLabels.map((labelKey, index) => (
          <div key={`${layoutType}-${index}`}>
            <label className="block text-xs text-text-secondary mb-1">
              {t(labelKey)}
            </label>
            <input
              type="text"
              value={paneCommands[index] ?? ""}
              onChange={(e) => handleCommandChange(index, e.target.value)}
              placeholder={t("commandPlaceholder")}
              className="w-full px-3 py-1.5 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
            />
          </div>
        ))}
      </div>

      {/* 액션 버튼 */}
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={handleSave}
          disabled={isPending}
          className="px-4 py-1.5 text-sm bg-brand-primary text-white rounded-md hover:bg-brand-primary/90 disabled:opacity-50 transition-colors"
        >
          {isPending ? t("saving") : tc("save")}
        </button>

        {!isGlobal && initialConfig?.id && (
          <button
            type="button"
            onClick={handleDelete}
            disabled={isPending}
            className="px-4 py-1.5 text-sm text-status-error border border-status-error rounded-md hover:bg-status-error/10 disabled:opacity-50 transition-colors"
          >
            {t("resetToGlobal")}
          </button>
        )}
      </div>

      {/* 메시지 */}
      {message && (
        <p className={`text-xs ${message.type === "success" ? "text-status-success" : "text-status-error"}`}>
          {message.text}
        </p>
      )}
    </div>
  );
}
