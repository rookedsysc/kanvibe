import { NextRequest, NextResponse } from "next/server";
import { getTaskRepository } from "@/lib/database";
import { KanbanTask, TaskStatus, SessionType } from "@/entities/KanbanTask";
import { createWorktreeWithSession } from "@/lib/worktree";
import { getProjectRepository } from "@/lib/database";
import { revalidatePath } from "next/cache";
import { broadcastBoardUpdate } from "@/lib/boardNotifier";

/**
 * Hook API: 작업 시작.
 * AI 에이전트가 작업을 시작할 때 호출하여 progress 카드를 자동 생성한다.
 */
export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const { title, branchName, agentType, sessionType, sshHost, projectId, baseBranch } = body;

    if (!title) {
      return NextResponse.json(
        { success: false, error: "title은 필수입니다." },
        { status: 400 }
      );
    }

    const repo = await getTaskRepository();

    const task = repo.create({
      title,
      branchName: branchName || null,
      agentType: agentType || null,
      sessionType: sessionType ? (sessionType as SessionType) : null,
      sshHost: sshHost || null,
      projectId: projectId || null,
      baseBranch: baseBranch || null,
      status: TaskStatus.PROGRESS,
    });

    if (branchName && sessionType && projectId) {
      try {
        const projectRepo = await getProjectRepository();
        const project = await projectRepo.findOneBy({ id: projectId });

        if (project) {
          const base = baseBranch || project.defaultBranch;
          const session = await createWorktreeWithSession(
            project.repoPath,
            branchName,
            base,
            sessionType as SessionType,
            project.sshHost,
            projectId
          );
          task.worktreePath = session.worktreePath;
          task.sessionName = session.sessionName;
          task.sshHost = project.sshHost;
        }
      } catch (error) {
        console.error("Worktree/세션 생성 실패:", error);
      }
    }

    const saved = await repo.save(task);
    revalidatePath("/[locale]", "page");
    broadcastBoardUpdate();

    return NextResponse.json({
      success: true,
      data: {
        id: saved.id,
        status: saved.status,
        sessionName: saved.sessionName,
      },
    });
  } catch (error) {
    console.error("Hook start 오류:", error);
    return NextResponse.json(
      { success: false, error: "서버 오류" },
      { status: 500 }
    );
  }
}
