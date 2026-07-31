import type { Project } from "@/entities/Project";
import { getTaskRepository } from "@/lib/database";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { parseProjectColor, readKanvibeProjectColor, writeKanvibeProjectColor } from "@/lib/kanvibeProjectState";

type ColorSyncProject = Pick<Project, "id" | "repoPath" | "sshHost" | "color">;

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * 프로젝트 색상을 `.kanvibe/project.json`에 기록해 같은 저장소를 보는 다른 KanVibe client와 공유한다.
 * 프로젝트 루트와 소속 task의 worktree에 모두 기록해 어느 경로로 열어도 같은 색상을 읽게 한다.
 */
export async function persistProjectColorToKanvibeState(project: ColorSyncProject): Promise<void> {
  const projectColor = parseProjectColor(project.color);
  if (!projectColor) {
    return;
  }

  /** worktree들은 git common dir의 info/exclude를 공유하므로 exclude 갱신은 저장소당 한 번이면 된다 */
  await excludeKanvibeStateDirectory(project);

  await Promise.all(
    (await resolveProjectColorRepoPaths(project)).map(async (repoPath) => {
      try {
        await writeKanvibeProjectColor(repoPath, projectColor, project.sshHost);
      } catch (error) {
        console.warn("[project-color] .kanvibe 색상 기록 실패", {
          repoPath,
          sshHost: project.sshHost ?? null,
          error: getErrorMessage(error),
        });
      }
    }),
  );
}

async function excludeKanvibeStateDirectory(project: ColorSyncProject): Promise<void> {
  try {
    await addAiToolPatternsToGitExclude(project.repoPath, project.sshHost);
  } catch (error) {
    console.warn("[project-color] git exclude 패턴 추가 실패", {
      repoPath: project.repoPath,
      sshHost: project.sshHost ?? null,
      error: getErrorMessage(error),
    });
  }
}

/**
 * 다른 client가 `.kanvibe/project.json`에 남긴 프로젝트 색상을 읽는다.
 * 프로젝트를 새로 등록할 때 기존 client가 정한 색상을 그대로 이어받기 위해 사용한다.
 */
export async function readSharedProjectColor(
  repoPath: string,
  sshHost?: string | null,
): Promise<string | null> {
  try {
    return await readKanvibeProjectColor(repoPath, sshHost);
  } catch (error) {
    console.warn("[project-color] .kanvibe 색상 읽기 실패", {
      repoPath,
      sshHost: sshHost ?? null,
      error: getErrorMessage(error),
    });
    return null;
  }
}

/**
 * 다른 client가 바꾼 색상을 프로젝트에 반영한다.
 * @returns 색상이 실제로 바뀌었으면 true
 */
export async function applySharedProjectColor(project: Project): Promise<boolean> {
  const sharedColor = await readSharedProjectColor(project.repoPath, project.sshHost);
  if (!sharedColor || sharedColor === project.color) {
    return false;
  }

  project.color = sharedColor;
  return true;
}

/** 색상을 기록할 저장소 경로 목록. 프로젝트 루트와 소속 task worktree를 포함한다 */
async function resolveProjectColorRepoPaths(project: ColorSyncProject): Promise<string[]> {
  const repoPaths = new Set<string>([project.repoPath]);

  try {
    const taskRepo = await getTaskRepository();
    const tasks = await taskRepo.findBy({ projectId: project.id });
    for (const task of tasks) {
      if (task.worktreePath) {
        repoPaths.add(task.worktreePath);
      }
    }
  } catch (error) {
    console.warn("[project-color] worktree 경로 조회 실패", {
      projectId: project.id,
      error: getErrorMessage(error),
    });
  }

  return [...repoPaths];
}
