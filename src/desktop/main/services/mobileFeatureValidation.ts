import { getTaskById } from "./kanbanService";
import { getProjectById } from "./projectService";

export class MobileFeatureError extends Error {
  constructor(readonly statusCode: 400 | 404 | 409, message: string) {
    super(message);
    this.name = "MobileFeatureError";
  }
}

export function readMobileObject(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new MobileFeatureError(400, "요청 값이 올바르지 않습니다");
  }
  return value as Record<string, unknown>;
}

export function readMobileText(value: unknown, label: string, maxLength = 4096): string {
  if (typeof value !== "string" || !value.trim() || value.length > maxLength || /[\x00-\x1f\x7f]/.test(value)) {
    throw new MobileFeatureError(400, `${label} 값이 올바르지 않습니다`);
  }
  return value.trim();
}

export function readMobileEnum<T extends string>(values: readonly T[], value: unknown, label: string): T {
  if (typeof value !== "string" || !values.includes(value as T)) {
    throw new MobileFeatureError(400, `${label} 값이 올바르지 않습니다`);
  }
  return value as T;
}

export function readMobileBoolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") throw new MobileFeatureError(400, `${label} 값이 올바르지 않습니다`);
  return value;
}

export async function requireMobileTask(taskId: string) {
  const task = await getTaskById(readMobileText(taskId, "태스크", 128));
  if (!task) throw new MobileFeatureError(404, "없는 태스크입니다");
  return task;
}

export async function requireMobileProject(projectId: string) {
  const project = await getProjectById(readMobileText(projectId, "프로젝트", 128));
  if (!project) throw new MobileFeatureError(404, "없는 프로젝트입니다");
  return project;
}
