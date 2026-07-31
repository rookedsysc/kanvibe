"use client";

interface ProjectIconProps {
  projectName: string;
  /** 프로젝트 등록 시 받아둔 GitHub repo/org 아이콘 data URL */
  iconDataUrl?: string | null;
  sizeClassName?: string;
}

/**
 * 프로젝트 제목 앞에 붙는 GitHub repo/org 아이콘.
 * GitHub 저장소가 아니거나 아이콘을 받지 못한 프로젝트에서는 아무것도 그리지 않는다.
 */
export default function ProjectIcon({
  projectName,
  iconDataUrl,
  sizeClassName = "h-3.5 w-3.5",
}: ProjectIconProps) {
  if (!iconDataUrl) {
    return null;
  }

  return (
    <img
      src={iconDataUrl}
      alt={`${projectName} icon`}
      className={`${sizeClassName} shrink-0 rounded-sm object-cover`}
      data-testid="project-github-icon"
    />
  );
}
