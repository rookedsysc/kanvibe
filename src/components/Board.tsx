"use client";

import { useState, useCallback, useEffect } from "react";
import { useTranslations } from "next-intl";
import { DragDropContext, type DropResult } from "@hello-pangea/dnd";
import Column from "./Column";
import CreateTaskModal from "./CreateTaskModal";
import ProjectSettings from "./ProjectSettings";
import TaskContextMenu from "./TaskContextMenu";
import BranchTaskModal from "./BranchTaskModal";
import { updateTaskStatus, reorderTasks, deleteTask } from "@/app/actions/kanban";
import type { TasksByStatus } from "@/app/actions/kanban";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { logoutAction } from "@/app/actions/auth";

interface BoardProps {
  initialTasks: TasksByStatus;
  sshHosts: string[];
  projects: Project[];
}

const COLUMNS: { status: TaskStatus; labelKey: string; colorClass: string }[] = [
  { status: TaskStatus.TODO, labelKey: "todo", colorClass: "bg-status-todo" },
  { status: TaskStatus.PROGRESS, labelKey: "progress", colorClass: "bg-status-progress" },
  { status: TaskStatus.REVIEW, labelKey: "review", colorClass: "bg-status-review" },
  { status: TaskStatus.DONE, labelKey: "done", colorClass: "bg-status-done" },
];

interface ContextMenuState {
  isOpen: boolean;
  x: number;
  y: number;
  task: KanbanTask | null;
}

export default function Board({ initialTasks, sshHosts, projects }: BoardProps) {
  const t = useTranslations("board");
  const tt = useTranslations("task");
  const tc = useTranslations("common");
  const [tasks, setTasks] = useState<TasksByStatus>(initialTasks);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isBranchModalOpen, setIsBranchModalOpen] = useState(false);
  const [isMounted, setIsMounted] = useState(false);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({
    isOpen: false,
    x: 0,
    y: 0,
    task: null,
  });

  useEffect(() => {
    setIsMounted(true);
  }, []);

  /** 서버 revalidation 후 initialTasks가 변경되면 로컬 state에 반영한다 */
  useEffect(() => {
    setTasks(initialTasks);
  }, [initialTasks]);

  const handleDragEnd = useCallback(
    (result: DropResult) => {
      const { source, destination, draggableId } = result;
      if (!destination) return;

      const sourceStatus = source.droppableId as TaskStatus;
      const destStatus = destination.droppableId as TaskStatus;

      setTasks((prev) => {
        const updated = { ...prev };

        const sourceTasks = [...updated[sourceStatus]];
        const [movedTask] = sourceTasks.splice(source.index, 1);

        if (sourceStatus === destStatus) {
          sourceTasks.splice(destination.index, 0, movedTask);
          updated[sourceStatus] = sourceTasks;

          reorderTasks(
            sourceStatus,
            sourceTasks.map((taskItem) => taskItem.id)
          );
        } else {
          const destTasks = [...updated[destStatus]];
          const updatedTask: KanbanTask = { ...movedTask, status: destStatus };
          destTasks.splice(destination.index, 0, updatedTask);

          updated[sourceStatus] = sourceTasks;
          updated[destStatus] = destTasks;

          updateTaskStatus(draggableId, destStatus);
        }

        return updated;
      });
    },
    []
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, task: KanbanTask) => {
      e.preventDefault();
      setContextMenu({ isOpen: true, x: e.clientX, y: e.clientY, task });
    },
    []
  );

  const handleCloseContextMenu = useCallback(() => {
    setContextMenu((prev) => ({ ...prev, isOpen: false, task: null }));
  }, []);

  const handleBranchFromCard = useCallback(() => {
    setIsBranchModalOpen(true);
    setContextMenu((prev) => ({ ...prev, isOpen: false }));
  }, []);

  const handleDeleteFromCard = useCallback(() => {
    if (contextMenu.task && confirm(tt("deleteConfirm"))) {
      deleteTask(contextMenu.task.id);
    }
    handleCloseContextMenu();
  }, [contextMenu.task, handleCloseContextMenu, tt]);

  return (
    <div className="min-h-screen">
      <header className="flex items-center justify-between px-6 py-4 border-b border-border-default bg-bg-surface">
        <h1 className="text-xl font-bold text-text-primary">{t("title")}</h1>
        <div className="flex items-center gap-3">
          <button
            onClick={() => setIsModalOpen(true)}
            className="px-4 py-1.5 text-sm bg-brand-primary hover:bg-brand-hover text-text-inverse rounded-md font-medium transition-colors"
          >
            {t("newTask")}
          </button>
          <button
            onClick={() => setIsSettingsOpen(true)}
            className="px-3 py-1.5 text-sm text-text-muted hover:text-text-primary transition-colors"
            title={tc("settings")}
          >
            &#9881;
          </button>
          <form action={logoutAction}>
            <button
              type="submit"
              className="px-3 py-1.5 text-sm text-text-muted hover:text-text-primary transition-colors"
            >
              {tc("logout")}
            </button>
          </form>
        </div>
      </header>

      <main className="p-6">
        {isMounted ? (
          <DragDropContext onDragEnd={handleDragEnd}>
            <div className="flex gap-4 overflow-x-auto">
              {COLUMNS.map((col) => (
                <Column
                  key={col.status}
                  status={col.status}
                  tasks={tasks[col.status]}
                  label={t(`columns.${col.labelKey}`)}
                  colorClass={col.colorClass}
                  onContextMenu={handleContextMenu}
                />
              ))}
            </div>
          </DragDropContext>
        ) : (
          <div className="flex gap-4 overflow-x-auto">
            {COLUMNS.map((col) => (
              <div key={col.status} className="flex-1 min-w-[280px] max-w-[350px]">
                <div className="flex items-center gap-2 mb-3 px-2">
                  <div className={`w-3 h-3 rounded-full ${col.colorClass}`} />
                  <h2 className="text-sm font-semibold text-text-secondary uppercase tracking-wide">
                    {t(`columns.${col.labelKey}`)}
                  </h2>
                </div>
                <div className="min-h-[200px] p-2 rounded-lg" />
              </div>
            ))}
          </div>
        )}
      </main>

      <CreateTaskModal
        isOpen={isModalOpen}
        onClose={() => setIsModalOpen(false)}
        sshHosts={sshHosts}
        projects={projects}
      />

      <ProjectSettings
        isOpen={isSettingsOpen}
        onClose={() => setIsSettingsOpen(false)}
        projects={projects}
        sshHosts={sshHosts}
      />

      {contextMenu.isOpen && contextMenu.task && (
        <TaskContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={handleCloseContextMenu}
          onBranch={handleBranchFromCard}
          onDelete={handleDeleteFromCard}
          hasBranch={!!contextMenu.task.branchName}
        />
      )}

      {isBranchModalOpen && contextMenu.task && (
        <BranchTaskModal
          task={contextMenu.task}
          projects={projects}
          onClose={() => {
            setIsBranchModalOpen(false);
            handleCloseContextMenu();
          }}
        />
      )}
    </div>
  );
}
