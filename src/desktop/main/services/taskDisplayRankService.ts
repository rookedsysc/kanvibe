import { MoreThan, type Repository } from "typeorm";
import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { rankBetween } from "@/desktop/shared/displayRank";

/**
 * 카드가 컬럼에서 차지할 자리(display rank)를 계산하는 한 곳.
 *
 * task를 INSERT 하는 경로는 kanbanService·projectService·hookService로 나뉘어 있는데,
 * 그중 하나라도 rank를 비워 두면 컬럼 DEFAULT 값이 그대로 들어가 같은 rank를 가진 카드가 겹친다.
 * 겹친 rank는 드롭 위치 계산을 무너뜨리고 스스로 풀리지 않으므로, 생성 경로가 모두 이 헬퍼를 거치게 한다.
 */

/** 해당 컬럼에서 가장 뒤에 있는 rank를 찾는다. 없으면 null이라 맨 앞부터 시작한다 */
export async function findLastDisplayRank(
  repo: Repository<KanbanTask>,
  status: TaskStatus,
): Promise<string | null> {
  const lastTask = await repo.findOne({
    where: { status },
    order: { displayRank: "DESC" },
    select: ["displayRank"],
  });

  return lastTask?.displayRank ?? null;
}

/**
 * 해당 컬럼에서 주어진 rank 바로 뒤에 오는 rank를 찾는다. 없으면 null이라 그 rank가 마지막이라는 뜻이다.
 * 화면 순서만으로는 알 수 없는 실제 rank 순서의 다음 자리를 찾을 때 쓴다.
 */
export async function findNextDisplayRank(
  repo: Repository<KanbanTask>,
  status: TaskStatus,
  afterRank: string,
): Promise<string | null> {
  const nextTask = await repo.findOne({
    where: { status, displayRank: MoreThan(afterRank) },
    order: { displayRank: "ASC" },
    select: ["displayRank"],
  });

  return nextTask?.displayRank ?? null;
}

/** 새 카드를 해당 컬럼 맨 뒤에 놓을 rank를 만든다. 모든 task 생성 경로가 이 값을 써야 rank가 겹치지 않는다 */
export async function appendDisplayRank(
  repo: Repository<KanbanTask>,
  status: TaskStatus,
): Promise<string> {
  return rankBetween(await findLastDisplayRank(repo, status), null);
}
