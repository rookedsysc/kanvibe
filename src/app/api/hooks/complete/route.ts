import { NextRequest, NextResponse } from "next/server";
import { getTaskRepository } from "@/lib/database";
import { TaskStatus } from "@/entities/KanbanTask";

/**
 * Hook API: 작업 완료.
 * AI 에이전트가 작업을 완료했을 때 호출하여 done 상태로 변경한다.
 */
export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const { id } = body;

    if (!id) {
      return NextResponse.json(
        { success: false, error: "id는 필수입니다." },
        { status: 400 }
      );
    }

    const repo = await getTaskRepository();
    const task = await repo.findOneBy({ id });

    if (!task) {
      return NextResponse.json(
        { success: false, error: "작업을 찾을 수 없습니다." },
        { status: 404 }
      );
    }

    task.status = TaskStatus.DONE;
    const saved = await repo.save(task);

    return NextResponse.json({
      success: true,
      data: { id: saved.id, status: saved.status },
    });
  } catch (error) {
    console.error("Hook complete 오류:", error);
    return NextResponse.json(
      { success: false, error: "서버 오류" },
      { status: 500 }
    );
  }
}
