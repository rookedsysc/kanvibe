"use client";

import { getReadableTextColor } from "@/lib/projectColor";

interface ProjectIconProps {
  projectName: string;
  /** 프로젝트 등록 시 받아둔 GitHub repo/org 아이콘 data URL */
  iconDataUrl?: string | null;
  /** 아이콘이 없을 때 이니셜 배지를 칠할 프로젝트 색상. 색상이 없으면 배지를 그리지 않는다 */
  color?: string | null;
  sizeClassName?: string;
}

function getProjectInitial(projectName: string): string {
  return [...projectName.trim()][0] ?? "";
}

/**
 * 프로젝트 제목 앞에 붙는 마커.
 * GitHub 아이콘을 받아둔 프로젝트는 아이콘을, 아이콘이 없는 프로젝트는 프로젝트 색상의
 * 이니셜 배지를 그린다. 바로 옆에 프로젝트명이 그대로 나오므로 배지는 스크린 리더에서 숨긴다.
 */
export default function ProjectIcon({
  projectName,
  iconDataUrl,
  color,
  sizeClassName = "h-3.5 w-3.5",
}: ProjectIconProps) {
  if (iconDataUrl) {
    return (
      <img
        src={iconDataUrl}
        alt={`${projectName} icon`}
        className={`${sizeClassName} shrink-0 rounded-sm object-cover`}
        data-testid="project-github-icon"
      />
    );
  }

  const initial = getProjectInitial(projectName);
  if (!color || !initial) {
    return null;
  }

  return (
    <span
      className={`${sizeClassName} inline-flex shrink-0 items-center justify-center rounded-sm text-[9px] font-bold uppercase leading-none`}
      style={{ backgroundColor: color, color: getReadableTextColor(color) }}
      aria-hidden="true"
      data-testid="project-initial-icon"
    >
      {initial}
    </span>
  );
}
