"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { useRouter } from "@/desktop/renderer/navigation";
import { HexColorPicker, HexColorInput } from "react-colorful";
import { updateProjectColor } from "@/desktop/renderer/actions/kanban";
import { computeProjectColor } from "@/lib/projectColor";
import { useTrackedAction } from "@/desktop/renderer/hooks/useTrackedAction";

const PRESET_COLORS = [
  "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
  "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];

interface ProjectColorEditorProps {
  projectId: string;
  projectName: string;
  currentColor: string | null;
}

export default function ProjectColorEditor({ projectId, projectName, currentColor }: ProjectColorEditorProps) {
  const tc = useTranslations("common");
  const router = useRouter();
  const [isOpen, setIsOpen] = useState(false);
  const fallbackColor = currentColor || computeProjectColor(projectName);
  const [color, setColor] = useState(fallbackColor);

  function handleOpen() {
    /** 팝오버를 열 때 최신 prop 값으로 동기화한다 */
    setColor(currentColor || computeProjectColor(projectName));
    setIsOpen(true);
  }

  /** 미리 바꿔 보인 색은 저장이 실패하면 저장된 색으로 되돌려, 저장되지 않은 색이 남아 보이지 않게 한다 */
  const colorSave = useTrackedAction(async (hex: string) => {
    try {
      await updateProjectColor(projectId, hex);
    } catch (error) {
      setColor(currentColor || computeProjectColor(projectName));
      throw error;
    }
    router.refresh();
  });
  const isPending = colorSave.isPending;

  function handleApply(hex: string) {
    setColor(hex);
    void colorSave.run(hex);
  }

  return (
    <div className="relative">
      {/* 색상 미리보기 버튼 */}
      <button
        type="button"
        onClick={handleOpen}
        disabled={isPending}
        aria-busy={isPending || undefined}
        className={`w-6 h-6 rounded-full border-2 border-border-default cursor-pointer transition-transform hover:scale-110 disabled:cursor-progress ${isPending ? "opacity-50" : ""}`}
        style={{ backgroundColor: color }}
        aria-label="Pick color"
      />

      {colorSave.error ? (
        <p role="alert" className="absolute right-0 top-8 z-30 w-[220px] text-[11px] text-status-error">
          {tc("saveFailed", { reason: colorSave.error })}
        </p>
      ) : null}

      {/* 색상 선택 팝오버 */}
      {isOpen && (
        <>
          {/* 백드롭 */}
          <div className="fixed inset-0 z-40" onClick={() => setIsOpen(false)} />

          <div className="absolute right-0 top-8 z-50 bg-bg-surface border border-border-default rounded-lg shadow-lg p-3 w-[220px]">
            {/* react-colorful 피커 */}
            <HexColorPicker
              color={color}
              onChange={setColor}
              style={{ width: "100%", height: "150px" }}
            />

            {/* Hex 입력 + OK 버튼 */}
            <div className="flex items-center gap-1.5 mt-2 min-w-0">
              <span className="text-xs text-text-muted shrink-0">#</span>
              <HexColorInput
                color={color}
                onChange={setColor}
                className="min-w-0 flex-1 text-xs px-1.5 py-1 rounded border border-border-default bg-bg-page text-text-primary font-mono"
                prefixed={false}
              />
              <button
                type="button"
                onClick={() => { handleApply(color); setIsOpen(false); }}
                disabled={isPending}
                className="shrink-0 text-xs px-2 py-1 rounded bg-brand-primary text-text-inverse hover:bg-brand-hover transition-colors disabled:opacity-50"
              >
                OK
              </button>
            </div>

            {/* 프리셋 색상 */}
            <div className="flex items-center gap-1.5 mt-2 flex-wrap">
              {PRESET_COLORS.map((preset) => (
                <button
                  key={preset}
                  type="button"
                  onClick={() => { handleApply(preset); setIsOpen(false); }}
                  className={`w-5 h-5 rounded-full cursor-pointer transition-all ${
                    color.toLowerCase() === preset.toLowerCase()
                      ? "ring-2 ring-offset-1 ring-text-primary scale-110"
                      : "hover:scale-110"
                  }`}
                  style={{ backgroundColor: preset }}
                  aria-label={`Color ${preset}`}
                />
              ))}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
