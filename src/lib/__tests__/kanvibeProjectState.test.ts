import { describe, expect, it, vi, beforeEach } from "vitest";
import { TaskStatus } from "@/entities/KanbanTask";

const { mockReadTextFile, mockReadTextFiles, mockWriteTextFile, mockWriteTextFileIfAbsent } = vi.hoisted(() => ({
  mockReadTextFile: vi.fn(),
  mockReadTextFiles: vi.fn(),
  mockWriteTextFile: vi.fn(),
  mockWriteTextFileIfAbsent: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  readTextFile: (...args: unknown[]) => mockReadTextFile(...args),
  readTextFiles: (...args: unknown[]) => mockReadTextFiles(...args),
  writeTextFile: (...args: unknown[]) => mockWriteTextFile(...args),
  writeTextFileIfAbsent: (...args: unknown[]) => mockWriteTextFileIfAbsent(...args),
}));

import {
  buildKanvibeProjectStateContent,
  buildKanvibeTargetsContent,
  buildKanvibeTaskDescriptionContent,
  buildKanvibeTaskStateContent,
  getKanvibeProjectStatePath,
  getKanvibeTargetsPath,
  getKanvibeTaskDescriptionPath,
  getKanvibeTaskStatePath,
  hasKanvibeHookTarget,
  parseKanvibeTargets,
  parseKanvibeTaskDescription,
  parseKanvibeTaskState,
  parseProjectColor,
  parseTaskStatus,
  readKanvibeProjectColor,
  readKanvibeTaskState,
  readKanvibeTaskSyncState,
  upsertKanvibeHookTarget,
  writeKanvibeProjectColor,
  writeKanvibeProjectColorIfAbsent,
  writeKanvibeTaskDescription,
  writeKanvibeTaskStatus,
} from "@/lib/kanvibeProjectState";

function getLastWrittenContent(): string {
  return mockWriteTextFile.mock.calls.at(-1)?.[1] as string;
}

describe("kanvibeProjectState", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReadTextFile.mockResolvedValue("");
    mockWriteTextFile.mockResolvedValue(undefined);
  });

  it("builds extensible status.json content without task/url mappings", () => {
    const content = buildKanvibeTaskStateContent({
      status: TaskStatus.REVIEW,
    });
    const parsed = JSON.parse(content) as { schemaVersion?: unknown; status?: unknown; updatedAt?: unknown };

    expect(parsed).toEqual({
      schemaVersion: 1,
      status: TaskStatus.REVIEW,
      updatedAt: expect.any(String),
    });
    expect(Number.isNaN(Date.parse(String(parsed.updatedAt)))).toBe(false);
    expect(content).not.toContain("task-1");
    expect(content).not.toContain("taskId");
    expect(content).not.toContain("url");
  });

  it("프로젝트 색상은 status.json이 아니라 project.json에 담는다", () => {
    expect(JSON.parse(buildKanvibeTaskStateContent({ status: TaskStatus.PROGRESS }))).toEqual({
      schemaVersion: 1,
      status: TaskStatus.PROGRESS,
      updatedAt: expect.any(String),
    });

    expect(JSON.parse(buildKanvibeProjectStateContent("#65D08A"))).toEqual({
      schemaVersion: 1,
      projectColor: "#65D08A",
      updatedAt: expect.any(String),
    });
  });

  it("`#RRGGBB` 형태의 색상만 통과시킨다", () => {
    expect(parseProjectColor("#65D08A")).toBe("#65D08A");
    expect(parseProjectColor("  #65d08a  ")).toBe("#65d08a");
    expect(parseProjectColor("65D08A")).toBeNull();
    expect(parseProjectColor("#65D08")).toBeNull();
    expect(parseProjectColor('#65D08A" ; rm -rf /')).toBeNull();
    expect(parseProjectColor(42)).toBeNull();
  });

  it("normalizes valid persisted task statuses and rejects unusable state files", () => {
    expect(parseTaskStatus("PROGRESS")).toBe(TaskStatus.PROGRESS);
    expect(parseTaskStatus(TaskStatus.PENDING)).toBe(TaskStatus.PENDING);
    expect(parseTaskStatus(42)).toBeNull();
    expect(parseTaskStatus("blocked")).toBeNull();

    expect(parseKanvibeTaskState("")).toBeNull();
    expect(parseKanvibeTaskState("not-a-status")).toBeNull();
    expect(parseKanvibeTaskState(JSON.stringify({ schemaVersion: 1, status: "review", updatedAt: "2026-06-03T00:00:00.000Z" }))).toEqual({
      schemaVersion: 1,
      status: TaskStatus.REVIEW,
      updatedAt: "2026-06-03T00:00:00.000Z",
    });
    expect(parseKanvibeTaskState("DONE\n")).toEqual({
      schemaVersion: 1,
      status: TaskStatus.DONE,
    });
    expect(parseKanvibeTaskState("  review  \n")).toEqual({
      schemaVersion: 1,
      status: TaskStatus.REVIEW,
    });
  });

  it("uses host-aware .kanvibe/status.json paths when reading and writing project status", async () => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({ schemaVersion: 1, status: "pending" }));
    await expect(readKanvibeTaskState("/remote/repo", "ssh-host")).resolves.toEqual({
      schemaVersion: 1,
      status: TaskStatus.PENDING,
    });
    expect(mockReadTextFile).toHaveBeenCalledWith("/remote/repo/.kanvibe/status.json", "ssh-host");

    await writeKanvibeTaskStatus("/remote/repo", TaskStatus.DONE, "ssh-host");
    expect(mockWriteTextFile).toHaveBeenLastCalledWith(
      "/remote/repo/.kanvibe/status.json",
      expect.stringContaining('"status": "done"'),
      "ssh-host",
    );
    const writtenContent = getLastWrittenContent();
    expect(JSON.parse(writtenContent)).toEqual({
      schemaVersion: 1,
      status: TaskStatus.DONE,
      updatedAt: expect.any(String),
    });
    expect(writtenContent).not.toContain("task-2");
    expect(writtenContent).not.toContain("taskId");
    expect(writtenContent).not.toContain("url");

    expect(getKanvibeTaskStatePath("/local/repo")).toBe("/local/repo/.kanvibe/status.json");
  });

  it("상태 기록과 색상 기록이 서로 다른 파일을 쓴다", async () => {
    await writeKanvibeTaskStatus("/local/repo", TaskStatus.REVIEW);
    expect(mockWriteTextFile).toHaveBeenLastCalledWith(
      "/local/repo/.kanvibe/status.json",
      expect.any(String),
      undefined,
    );
    expect(getLastWrittenContent()).not.toContain("projectColor");

    await writeKanvibeProjectColor("/local/repo", "#0064FF");
    expect(mockWriteTextFile).toHaveBeenLastCalledWith(
      "/local/repo/.kanvibe/project.json",
      expect.any(String),
      undefined,
    );
    expect(getLastWrittenContent()).not.toContain("status");
  });

  /** 상태 기록은 색상 파일을 읽지도 쓰지도 않으므로 동시 갱신이 서로를 되돌릴 수 없다 */
  it("상태 기록은 읽기 없이 status.json만 덮어쓴다", async () => {
    await writeKanvibeTaskStatus("/local/repo", TaskStatus.REVIEW);

    expect(mockReadTextFile).not.toHaveBeenCalled();
    expect(JSON.parse(getLastWrittenContent())).toEqual({
      schemaVersion: 1,
      status: TaskStatus.REVIEW,
      updatedAt: expect.any(String),
    });
  });

  it("색상은 host 별 project.json 경로에서 읽고 쓴다", async () => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({ schemaVersion: 1, projectColor: "#0064FF" }));
    await expect(readKanvibeProjectColor("/remote/repo", "ssh-host")).resolves.toBe("#0064FF");
    expect(mockReadTextFile).toHaveBeenCalledWith("/remote/repo/.kanvibe/project.json", "ssh-host");

    expect(getKanvibeProjectStatePath("/local/repo")).toBe("/local/repo/.kanvibe/project.json");
  });

  it("형식이 잘못된 색상은 기록하지 않는다", async () => {
    await writeKanvibeProjectColor("/local/repo", "not-a-color");

    expect(mockWriteTextFile).not.toHaveBeenCalled();
  });

  /** 씨앗 기록은 권위 파일을 만드는 것이지 이미 있는 값을 바꾸는 것이 아니다 */
  it("씨앗 색상은 파일이 없을 때만 기록하는 경로로 쓴다", async () => {
    await writeKanvibeProjectColorIfAbsent("/local/repo", "#0064FF");

    expect(mockWriteTextFileIfAbsent).toHaveBeenLastCalledWith(
      "/local/repo/.kanvibe/project.json",
      expect.stringContaining("#0064FF"),
      undefined,
    );
    expect(mockWriteTextFile).not.toHaveBeenCalled();
  });

  it("형식이 어긋난 씨앗 색상은 기록하지 않는다", async () => {
    await writeKanvibeProjectColorIfAbsent("/local/repo", "not-a-color");

    expect(mockWriteTextFileIfAbsent).not.toHaveBeenCalled();
  });

  it("builds and parses .kanvibe/targets.json as client url to task id mappings", () => {
    const content = buildKanvibeTargetsContent([
      { url: "http://127.0.0.1:19736/", taskId: "task-local" },
      { url: "http://127.0.0.1:19736", taskId: "task-dev" },
      { url: "http://10.0.0.5:19736", taskId: "task-remote" },
    ]);
    const parsed = JSON.parse(content) as { schemaVersion?: unknown; targets?: unknown; updatedAt?: unknown };

    /** 같은 client(url)는 하나로 합치고, 다른 client는 그대로 남긴다 */
    expect(parsed).toEqual({
      schemaVersion: 1,
      targets: [
        { url: "http://127.0.0.1:19736", taskId: "task-local" },
        { url: "http://10.0.0.5:19736", taskId: "task-remote" },
      ],
      updatedAt: expect.any(String),
    });
    expect(parseKanvibeTargets(content)?.targets).toEqual(parsed.targets);
    expect(parseKanvibeTargets("not-json")).toEqual({ schemaVersion: 1, targets: [] });
  });

  it("같은 task를 보는 다른 client도 알림 대상으로 함께 남긴다", async () => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({
      schemaVersion: 1,
      targets: [{ url: "http://10.0.0.5:19736", taskId: "shared-task" }],
    }));

    await upsertKanvibeHookTarget(
      "/remote/repo",
      { url: "http://127.0.0.1:19736/", taskId: "shared-task" },
      "ssh-host",
    );

    expect(mockReadTextFile).toHaveBeenCalledWith("/remote/repo/.kanvibe/targets.json", "ssh-host");
    expect(JSON.parse(getLastWrittenContent())).toEqual({
      schemaVersion: 1,
      targets: [
        { url: "http://10.0.0.5:19736", taskId: "shared-task" },
        { url: "http://127.0.0.1:19736", taskId: "shared-task" },
      ],
      updatedAt: expect.any(String),
    });
  });

  it("같은 client가 다른 task로 재설치하면 taskId만 교체한다", async () => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({
      schemaVersion: 1,
      targets: [
        { url: "http://127.0.0.1:19736", taskId: "old-task" },
        { url: "http://10.0.0.5:19736", taskId: "other-client-task" },
      ],
    }));

    await upsertKanvibeHookTarget("/local/repo", { url: "http://127.0.0.1:19736", taskId: "new-task" });

    expect(JSON.parse(getLastWrittenContent())).toEqual({
      schemaVersion: 1,
      targets: [
        { url: "http://127.0.0.1:19736", taskId: "new-task" },
        { url: "http://10.0.0.5:19736", taskId: "other-client-task" },
      ],
      updatedAt: expect.any(String),
    });
    expect(getKanvibeTargetsPath("/local/repo")).toBe("/local/repo/.kanvibe/targets.json");
  });

  it("client url과 taskId 쌍이 모두 맞아야 등록된 대상으로 본다", () => {
    const content = JSON.stringify({
      schemaVersion: 1,
      targets: [{ url: "http://127.0.0.1:19736", taskId: "task-1" }],
    });

    expect(hasKanvibeHookTarget(content, { url: "http://127.0.0.1:19736/", taskId: "task-1" })).toBe(true);
    expect(hasKanvibeHookTarget(content, { url: "http://127.0.0.1:19736", taskId: "task-2" })).toBe(false);
    expect(hasKanvibeHookTarget(content, { url: "http://10.0.0.5:19736", taskId: "task-1" })).toBe(false);
    expect(hasKanvibeHookTarget("", { url: "http://127.0.0.1:19736", taskId: "task-1" })).toBe(false);
  });

  it("task 설명을 status.json이 아닌 task.json에 기록한다", async () => {
    await writeKanvibeTaskDescription("/local/repo", "결제 실패 로그 원인 추적");

    expect(mockWriteTextFile).toHaveBeenCalledWith(
      "/local/repo/.kanvibe/task.json",
      expect.any(String),
      undefined,
    );
    expect(JSON.parse(getLastWrittenContent())).toEqual({
      schemaVersion: 1,
      description: "결제 실패 로그 원인 추적",
      updatedAt: expect.any(String),
    });
    expect(getKanvibeTaskDescriptionPath("/local/repo")).toBe("/local/repo/.kanvibe/task.json");
    expect(getKanvibeTaskDescriptionPath("/remote/repo", "build-host")).toBe("/remote/repo/.kanvibe/task.json");
  });

  it("설명을 지우면 null을 기록해 다른 기기에서도 지워지게 한다", () => {
    const content = buildKanvibeTaskDescriptionContent(null);

    expect(JSON.parse(content)).toEqual({
      schemaVersion: 1,
      description: null,
      updatedAt: expect.any(String),
    });
    expect(parseKanvibeTaskDescription(content)?.description).toBeNull();
  });

  it("설명 파일이 없거나 형식이 깨지면 정보 없음으로 본다", () => {
    expect(parseKanvibeTaskDescription("")).toBeNull();
    expect(parseKanvibeTaskDescription("not json")).toBeNull();
    expect(parseKanvibeTaskDescription(JSON.stringify({ schemaVersion: 1 }))).toBeNull();
    expect(parseKanvibeTaskDescription(JSON.stringify({ description: 123 }))).toBeNull();
  });

  it("상태와 설명을 원격 왕복 한 번으로 함께 읽어온다", async () => {
    mockReadTextFiles.mockResolvedValue(new Map([
      ["/local/repo/.kanvibe/status.json", { exists: true, content: JSON.stringify({ status: "review" }) }],
      ["/local/repo/.kanvibe/task.json", { exists: true, content: JSON.stringify({ description: "결제 실패 로그 원인 추적" }) }],
    ]));

    const syncState = await readKanvibeTaskSyncState("/local/repo");

    expect(syncState.status).toBe(TaskStatus.REVIEW);
    expect(syncState.description?.description).toBe("결제 실패 로그 원인 추적");
    expect(mockReadTextFiles).toHaveBeenCalledTimes(1);
    expect(mockReadTextFiles).toHaveBeenCalledWith(
      ["/local/repo/.kanvibe/status.json", "/local/repo/.kanvibe/task.json"],
      undefined,
    );
  });

  it("상태나 설명 파일이 없으면 각각 기록 없음으로 돌려준다", async () => {
    mockReadTextFiles.mockResolvedValue(new Map([
      ["/local/repo/.kanvibe/status.json", { exists: true, content: JSON.stringify({ status: "review" }) }],
      ["/local/repo/.kanvibe/task.json", { exists: false, content: "" }],
    ]));

    expect(await readKanvibeTaskSyncState("/local/repo")).toEqual({
      status: TaskStatus.REVIEW,
      description: null,
    });
  });
});
