import { NextRequest, NextResponse } from "next/server";
import { getTaskRepository, getProjectRepository } from "@/lib/database";
import { TaskStatus } from "@/entities/KanbanTask";
import { cleanupTaskResources } from "@/app/actions/kanban";
import { revalidatePath } from "next/cache";
import { broadcastBoardUpdate } from "@/lib/boardNotifier";

const STATUS_MAP: Record<string, TaskStatus> = {
  todo: TaskStatus.TODO,
  progress: TaskStatus.PROGRESS,
  pending: TaskStatus.PENDING,
  review: TaskStatus.REVIEW,
  done: TaskStatus.DONE,
};

/**
 * Hook API: branchName + projectName 기반 작업 상태 업데이트.
 * Claude Code hooks에서 호출하여 현재 브랜치의 작업 상태를 자동 변경한다.
 */
export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const { branchName, projectName, status } = body;

    if (!branchName || !projectName || !status) {
      return NextResponse.json(
        { success: false, error: "branchName, projectName, status는 필수입니다." },
        { status: 400 }
      );
    }

    const taskStatus = STATUS_MAP[status.toLowerCase()];
    if (!taskStatus) {
      return NextResponse.json(
        { success: false, error: `유효하지 않은 상태입니다: ${status}` },
        { status: 400 }
      );
    }

    const projectRepo = await getProjectRepository();
    const project = await projectRepo.findOneBy({ name: projectName });

    if (!project) {
      return NextResponse.json(
        { success: false, error: `프로젝트를 찾을 수 없습니다: ${projectName}` },
        { status: 404 }
      );
    }

    const taskRepo = await getTaskRepository();
    const task = await taskRepo.findOneBy({
      branchName,
      projectId: project.id,
    });

    if (!task) {
      return NextResponse.json(
        { success: false, error: `작업을 찾을 수 없습니다: ${projectName}/${branchName}` },
        { status: 404 }
      );
    }

    if (taskStatus === TaskStatus.DONE) {
      await cleanupTaskResources(task);
      task.sessionType = null;
      task.sessionName = null;
      task.worktreePath = null;
      task.sshHost = null;
    }

    task.status = taskStatus;
    const saved = await taskRepo.save(task);
    revalidatePath("/[locale]", "page");
    broadcastBoardUpdate();

    return NextResponse.json({
      success: true,
      data: { id: saved.id, status: saved.status, branchName, projectName },
    });
  } catch (error) {
    console.error("Hook status 오류:", error);
    return NextResponse.json(
      { success: false, error: "서버 오류" },
      { status: 500 }
    );
  }
}
