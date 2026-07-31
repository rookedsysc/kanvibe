import type { Project } from "@/entities/Project";
import { getProjectRepository, getTaskRepository } from "@/lib/database";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import {
  parseProjectColor,
  readKanvibeProjectColor,
  writeKanvibeProjectColor,
  writeKanvibeProjectColorIfAbsent,
} from "@/lib/kanvibeProjectState";

/**
 * 프로젝트 색상의 권위는 프로젝트 루트의 `.kanvibe/project.json` 하나뿐이다.
 *
 * 루트 파일을 쓰는 주체는 사용자 색상 편집과, 파일이 아직 없을 때의 씨앗 기록 둘로 한정한다.
 * background sync는 루트를 읽기만 하고 worktree 전파와 DB 반영만 담당한다.
 * sync가 루트를 쓰게 두면 sync 시작 시점에 읽은 낡은 색상이 그 사이의 사용자 편집을 되돌리고,
 * 되돌아간 루트 값이 다음 sync에서 다시 DB로 내려가 색이 왕복하게 된다.
 */

type ColorSyncProject = Pick<Project, "id" | "repoPath" | "sshHost" | "color">;

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * 사용자가 고른 색상을 `.kanvibe/project.json`에 기록해 같은 저장소를 보는 다른 KanVibe client와 공유한다.
 * 권위인 프로젝트 루트를 먼저 확정한 뒤 소속 task worktree에 퍼뜨려, 어느 경로로 열어도 같은 색상을 읽게 한다.
 */
export async function persistProjectColorToKanvibeState(project: ColorSyncProject): Promise<void> {
  const projectColor = parseProjectColor(project.color);
  if (!projectColor) {
    return;
  }

  /** worktree들은 git common dir의 info/exclude를 공유하므로 exclude 갱신은 저장소당 한 번이면 된다 */
  await excludeKanvibeStateDirectory(project);

  await writeProjectColorQuietly(project.repoPath, projectColor, project.sshHost);

  const worktreePaths = (await resolveProjectColorRepoPaths(project))
    .filter((repoPath) => repoPath !== project.repoPath);
  await Promise.all(
    worktreePaths.map((repoPath) => writeProjectColorQuietly(repoPath, projectColor, project.sshHost)),
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
 * 권위인 프로젝트 루트의 공유 색상을 읽어 worktree에 퍼뜨리고, DB에 반영할 값을 돌려준다.
 * 공유 파일이 아직 없으면 현재 색상을 씨앗으로 기록한다. 씨앗이 없으면 이 기능 이전에 등록된
 * 프로젝트끼리는 서로 다른 DB 색상을 계속 유지하게 된다.
 *
 * 프로젝트 색상을 여기서 직접 바꾸지 않고 값만 돌려주는 이유는, 호출한 쪽이 DB에 쓸 때
 * 자신이 읽어둔 색상이 아직 최신인지 확인(compare-and-set)할 수 있어야 하기 때문이다.
 * @returns DB에 반영해야 할 공유 색상. 반영할 변경이 없으면 null
 */
export async function syncProjectColorWithKanvibeState(project: Project): Promise<string | null> {
  const sharedColor = await readSharedProjectColor(project.repoPath, project.sshHost);
  if (!sharedColor) {
    await seedSharedProjectColor(project);
    return null;
  }

  await propagateSharedProjectColorToWorktrees(project, sharedColor);

  return sharedColor === project.color ? null : sharedColor;
}

/**
 * 공유 색상 파일이 없는 프로젝트에 현재 색상을 씨앗으로 남긴다.
 *
 * 기록할 색상은 sync가 시작될 때 읽은 메모리 값이 아니라 DB의 현재 값이다. 그 사이 사용자가
 * 색을 바꿨다면 메모리 값은 이미 낡았고, 그 낡은 값이 권위 파일의 초기값으로 굳으면
 * 다음 sync가 방금 고른 색을 되돌린다.
 *
 * 기록 자체도 파일이 없을 때만 이뤄져야 한다. 존재 검사와 기록 사이에 사용자 편집이
 * 루트 파일을 만들 수 있으므로, 원자적 쓰기로 이미 만들어진 권위 파일을 덮지 않는다.
 */
async function seedSharedProjectColor(project: ColorSyncProject): Promise<void> {
  const projectColor = parseProjectColor(await readCurrentProjectColor(project.id) ?? project.color);
  if (!projectColor) {
    return;
  }

  await excludeKanvibeStateDirectory(project);

  try {
    await writeKanvibeProjectColorIfAbsent(project.repoPath, projectColor, project.sshHost);
  } catch (error) {
    console.warn("[project-color] .kanvibe 색상 씨앗 기록 실패", {
      repoPath: project.repoPath,
      sshHost: project.sshHost ?? null,
      error: getErrorMessage(error),
    });
  }
}

/** 씨앗으로 남길 색상은 sync 시작 시점의 스냅샷이 아니라 DB에 저장된 현재 값이어야 한다 */
async function readCurrentProjectColor(projectId: string): Promise<string | null> {
  try {
    const projectRepo = await getProjectRepository();
    return (await projectRepo.findOneBy({ id: projectId }))?.color ?? null;
  } catch (error) {
    console.warn("[project-color] 프로젝트 색상 조회 실패", {
      projectId,
      error: getErrorMessage(error),
    });
    return null;
  }
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
