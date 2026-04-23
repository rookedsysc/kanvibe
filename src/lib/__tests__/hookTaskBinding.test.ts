import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, readFile, rm } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";
import {
  buildShellTaskIdResolver,
  getHookTaskIdFilePath,
  readHookTaskIdFile,
  writeHookTaskIdFile,
} from "@/lib/hookTaskBinding";

describe("hookTaskBinding", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "hook-task-binding-"));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it("작업 ID 파일을 쓰면 개행 포함 원본과 trim된 바인딩 값을 함께 유지한다", async () => {
    // Given
    const filePath = getHookTaskIdFilePath(tempDir);

    // When
    await writeHookTaskIdFile(tempDir, "task-123");
    const stored = await readFile(filePath, "utf-8");
    const taskId = await readHookTaskIdFile(tempDir);

    // Then
    expect(stored).toBe("task-123\n");
    expect(taskId).toBe("task-123");
  });

  it("빈 작업 ID 파일은 바인딩되지 않은 상태로 처리한다", async () => {
    // Given
    await writeHookTaskIdFile(tempDir, "");

    // When
    const taskId = await readHookTaskIdFile(tempDir);

    // Then
    expect(taskId).toBeNull();
  });

  it("shell resolver는 파일 task id를 기본값보다 우선하도록 생성된다", () => {
    // Given

    // When
    const resolver = buildShellTaskIdResolver("fallback-task");

    // Then
    expect(resolver).toContain('TASK_ID_FILE=".kanvibe/task-id"');
    expect(resolver).toContain('TASK_ID="fallback-task"');
    expect(resolver).toContain('if [ -f "$TASK_ID_FILE" ]; then');
    expect(resolver).toContain('FILE_TASK_ID=$(tr -d \'\\r\\n\' < "$TASK_ID_FILE")');
    expect(resolver).toContain('TASK_ID="$FILE_TASK_ID"');
  });
});
