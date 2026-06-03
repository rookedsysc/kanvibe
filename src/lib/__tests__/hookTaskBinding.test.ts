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

  it("shell script에서 고정된 task id를 읽고 없으면 null을 반환한다", () => {
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
    expect(extractShellTaskId('#!/bin/bash\necho "$TASK_ID"')).toBeNull();
  });

  it("shell task id resolver는 double-quoted shell 특수 문자를 escape하고 다시 읽을 수 있다", () => {
    // Given
    const taskId = 'task-"quoted"-\\-`cmd`-$HOME';

    // When
    const resolver = buildShellTaskIdResolver(taskId);

    // Then
    expect(resolver).toBe('TASK_ID="task-\\"quoted\\"-\\\\-\\`cmd\\`-\\$HOME"');
    expect(extractShellTaskId(resolver)).toBe(taskId);
  });

  it("모든 shell hook이 같은 현재 task id를 가리키는지 판정한다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-123");
    const statusWithoutExpectedTask = getShellTaskIdBindingStatus(scripts);

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: true,
      hasExpectedTaskId: true,
      boundTaskId: "task-123",
    });
    expect(statusWithoutExpectedTask).toEqual({
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

  it("shell hook끼리 서로 다른 task id를 가지거나 비어 있으면 binding 판정을 실패시킨다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-999"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-123");
    const emptyStatus = getShellTaskIdBindingStatus([], "task-123");

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: false,
      hasExpectedTaskId: false,
      boundTaskId: null,
    });
    expect(emptyStatus).toEqual({
      hasTaskIdBinding: false,
      hasExpectedTaskId: false,
      boundTaskId: null,
    });
  });
});
