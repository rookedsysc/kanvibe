import type { Project } from "@/entities/Project";

/** 프로젝트 이름/경로/SSH host 중 하나라도 검색어를 포함하는지 판별한다 */
export function matchesProjectSearch(project: Project, query: string) {
  const normalizedQuery = query.trim().toLowerCase();

  if (!normalizedQuery) {
    return true;
  }

  return [project.name, project.repoPath, project.sshHost ?? ""]
    .some((value) => value.toLowerCase().includes(normalizedQuery));
}
