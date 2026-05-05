import { describe, expect, it } from "vitest";
import {
  buildShellTaskIdResolver,
  extractShellTaskId,
  getShellTaskIdBindingStatus,
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

  it("모든 shell hook이 같은 현재 task id를 가리키는지 판정한다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-123");

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: true,
      hasExpectedTaskId: true,
      boundTaskId: "task-123",
    });
  });

  it("shell hook의 task id가 현재 task와 다르면 expected 판정을 실패시킨다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-999");

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: true,
      hasExpectedTaskId: false,
      boundTaskId: "task-123",
    });
  });

  it("shell hook끼리 서로 다른 task id를 가지면 binding 판정을 실패시킨다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-999"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-123");

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: false,
      hasExpectedTaskId: false,
      boundTaskId: null,
    });
  });
});
