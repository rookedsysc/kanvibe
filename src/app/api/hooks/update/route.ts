import { NextRequest, NextResponse } from "next/server";
import { getTaskRepository } from "@/lib/database";
import { TaskStatus } from "@/entities/KanbanTask";

/**
 * Hook API: 작업 업데이트.
 * AI 에이전트가 작업 상태나 정보를 업데이트할 때 호출한다.
 */
export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const { id, status, title, description } = body;

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

    if (status) task.status = status as TaskStatus;
    if (title) task.title = title;
    if (description !== undefined) task.description = description;

    const saved = await repo.save(task);

    return NextResponse.json({
      success: true,
      data: { id: saved.id, status: saved.status },
    });
  } catch (error) {
    console.error("Hook update 오류:", error);
    return NextResponse.json(
      { success: false, error: "서버 오류" },
      { status: 500 }
    );
  }
}
