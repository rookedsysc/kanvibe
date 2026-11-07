import type { Project } from "@/entities/Project";
import { fuzzyMatch } from "@/utils/fuzzySearch";

/** 프로젝트 이름/경로/SSH host 중 하나라도 검색어의 문자 순서와 일치하는지 판별한다 */
export function matchesProjectSearch(project: Project, query: string) {
  const normalizedQuery = query.trim().toLowerCase();

  if (!normalizedQuery) {
    return true;
  }

  return [project.name, project.repoPath, project.sshHost ?? ""]
    .some((value) => fuzzyMatch(normalizedQuery, value) !== null);
}
