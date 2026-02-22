"use client";

import { useState, useTransition } from "react";
import { useRouter } from "@/i18n/navigation";
import { HexColorPicker, HexColorInput } from "react-colorful";
import { updateProjectColor } from "@/app/actions/kanban";

const PRESET_COLORS = [
  "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
  "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];

interface ProjectColorEditorProps {
  projectId: string;
  currentColor: string | null;
}

export default function ProjectColorEditor({ projectId, currentColor }: ProjectColorEditorProps) {
  const [isPending, startTransition] = useTransition();
  const router = useRouter();
  const [isOpen, setIsOpen] = useState(false);
  const [color, setColor] = useState(currentColor || PRESET_COLORS[0]);

  function handleOpen() {
    /** 팝오버를 열 때 최신 prop 값으로 동기화한다 */
    setColor(currentColor || PRESET_COLORS[0]);
    setIsOpen(true);
  }

  function handleApply(hex: string) {
    setColor(hex);
    startTransition(async () => {
      await updateProjectColor(projectId, hex);
      router.refresh();
    });
  }

  return (
    <div className="relative">
      {/* 색상 미리보기 버튼 */}
      <button
        type="button"
        onClick={handleOpen}
        className={`w-6 h-6 rounded-full border-2 border-border-default cursor-pointer transition-transform hover:scale-110 ${isPending ? "opacity-50" : ""}`}
        style={{ backgroundColor: color }}
        aria-label="Pick color"
      />

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
