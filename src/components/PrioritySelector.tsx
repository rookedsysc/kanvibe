"use client";

import { TaskPriority } from "@/entities/TaskPriority";
import { useTranslations } from "next-intl";

interface PrioritySelectorProps {
  value: TaskPriority | null;
  onChange: (priority: TaskPriority | null) => void;
}

const PRIORITY_OPTIONS: { value: TaskPriority | null; labelKey: string; colorClass: string }[] = [
  { value: null, labelKey: "priorityNone", colorClass: "bg-bg-page text-text-muted border-border-default" },
  { value: TaskPriority.LOW, labelKey: "priorityLow", colorClass: "bg-priority-low-bg text-priority-low-text border-priority-low-text/30" },
  { value: TaskPriority.MEDIUM, labelKey: "priorityMedium", colorClass: "bg-priority-medium-bg text-priority-medium-text border-priority-medium-text/30" },
  { value: TaskPriority.HIGH, labelKey: "priorityHigh", colorClass: "bg-priority-high-bg text-priority-high-text border-priority-high-text/30" },
];

export default function PrioritySelector({ value, onChange }: PrioritySelectorProps) {
  const t = useTranslations("task");

  return (
    <div className="flex items-center gap-1.5">
      {PRIORITY_OPTIONS.map((option) => {
        const isSelected = value === option.value;
        return (
          <button
            key={option.value ?? "none"}
            type="button"
            onClick={() => onChange(option.value)}
            className={`px-2.5 py-1 text-xs font-medium rounded-md border transition-all ${
              option.colorClass
            } ${
              isSelected
                ? "ring-2 ring-offset-1 ring-brand-primary shadow-sm"
                : "opacity-60 hover:opacity-100"
            }`}
          >
            {t(option.labelKey)}
          </button>
        );
      })}
    </div>
  );
}
