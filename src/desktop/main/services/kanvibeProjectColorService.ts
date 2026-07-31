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
    (await resolveProjectColorRepoPaths(project)).map(
      (repoPath) => writeProjectColorQuietly(repoPath, projectColor, project.sshHost),
    ),
  );
}

/** 한 경로의 색상 기록 실패가 나머지 경로 기록을 막지 않도록 경고만 남긴다 */
async function writeProjectColorQuietly(
  repoPath: string,
  projectColor: string,
  sshHost?: string | null,
): Promise<void> {
  try {
    await writeKanvibeProjectColor(repoPath, projectColor, sshHost);
  } catch (error) {
    console.warn("[project-color] .kanvibe 색상 기록 실패", {
      repoPath,
      sshHost: sshHost ?? null,
      error: getErrorMessage(error),
    });
  }
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
 * 프로젝트 색상을 `.kanvibe/project.json`과 양방향으로 맞춘다.
 * 공유 색상이 있으면 그 값을 프로젝트에 반영하고, 공유 파일이 아직 없으면 현재 색상을 씨앗으로 기록한다.
 * 씨앗 기록이 없으면 이 기능 이전에 등록된 프로젝트끼리는 서로 다른 DB 색상을 계속 유지하게 된다.
 * 색상이 이미 확정된 뒤에 생긴 worktree도 sync를 거치면 같은 색상 파일을 갖게 된다.
 * @returns 색상이 실제로 바뀌어 DB에 저장해야 하면 true
 */
export async function syncProjectColorWithKanvibeState(project: Project): Promise<boolean> {
  const sharedColor = await readSharedProjectColor(project.repoPath, project.sshHost);
  if (!sharedColor) {
    await persistProjectColorToKanvibeState(project);
    return false;
  }

  await propagateSharedProjectColorToWorktrees(project, sharedColor);

  if (sharedColor === project.color) {
    return false;
  }

  project.color = sharedColor;
  return true;
}

/**
 * 프로젝트 루트에 기록된 공유 색상을 소속 worktree들에 퍼뜨린다.
 * 색상이 확정된 뒤에 생긴 worktree는 공유 파일을 받지 못한 채 남아, 그 경로를 직접 등록한
 * 다른 client가 다른 색상을 계산하게 된다.
 *
 * 기록하는 값은 방금 루트에서 읽은 색상이다. 메모리의 프로젝트 색상은 sync가 시작될 때 읽은
 * 값이라 그 사이 사용자가 색을 바꿨으면 이미 낡았고, 그 낡은 값을 퍼뜨리면 방금 고른 색이 밀린다.
 * 루트 파일은 사용자 편집과 씨앗 기록만 쓰는 권위 경로이므로 여기서는 건드리지 않는다.
 */
async function propagateSharedProjectColorToWorktrees(
  project: ColorSyncProject,
  sharedColor: string,
): Promise<void> {
  const worktreePaths = (await resolveProjectColorRepoPaths(project))
    .filter((repoPath) => repoPath !== project.repoPath);

  const outdatedPaths = (await Promise.all(
    worktreePaths.map(async (repoPath) => (
      await readSharedProjectColor(repoPath, project.sshHost) === sharedColor ? null : repoPath
    )),
  )).filter((repoPath): repoPath is string => repoPath !== null);

  /** 이미 같은 색상인 경로만 남으면 sync 주기마다 파일을 다시 쓰지 않는다 */
  if (outdatedPaths.length === 0) {
    return;
  }

  await excludeKanvibeStateDirectory(project);
  await Promise.all(
    outdatedPaths.map((repoPath) => writeProjectColorQuietly(repoPath, sharedColor, project.sshHost)),
  );
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
