import type { Project } from "@/entities/Project";

/** worktree repoPath에서 메인 프로젝트 경로를 추출한다 */
function extractMainRepoPath(repoPath: string): string | null {
  const worktreeIndex = repoPath.indexOf("__worktrees");
  if (worktreeIndex === -1) return null;
  return repoPath.slice(0, worktreeIndex);
}

export function buildProjectDisplayMap(projects: Project[]) {
  const mainProjectsByLocation = new Map(
    projects.filter((project) => !extractMainRepoPath(project.repoPath)).map((project) => [
      JSON.stringify([project.sshHost, project.repoPath]), project,
    ]),
  );
  const displayMap = new Map<string, { name: string; groupKey: string; project: Project | undefined }>();
  for (const project of projects) {
    const mainPath = extractMainRepoPath(project.repoPath);
    const groupKey = JSON.stringify([project.sshHost, mainPath ?? project.repoPath]);
    const mainProject = mainProjectsByLocation.get(groupKey);
    displayMap.set(project.id, {
      name: mainProject?.name ?? (mainPath ? mainPath.split("/").pop() || project.name : project.name),
      groupKey,
      project: mainProject,
    });
  }
  return displayMap;
}
