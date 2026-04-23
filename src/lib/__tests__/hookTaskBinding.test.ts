import { describe, expect, it } from "vitest";
import {
  buildShellTaskIdResolver,
  extractShellTaskId,
} from "@/lib/hookTaskBinding";

describe("hookTaskBinding", () => {
  it("shell resolver는 hook 파일 안에 task id를 직접 고정한다", () => {
    // Given

    // When
    const resolver = buildShellTaskIdResolver("fallback-task");

    // Then
    expect(resolver).toBe('TASK_ID="fallback-task"');
    expect(resolver).not.toContain(".kanvibe/task-id");
  });

  it("shell script에서 고정된 task id를 읽는다", () => {
    // Given
    const script = [
      "#!/bin/bash",
      'TASK_ID="task-123"',
      'echo "$TASK_ID"',
    ].join("\n");

    // When
    const taskId = extractShellTaskId(script);

    // Then
    expect(taskId).toBe("task-123");
  });
});
