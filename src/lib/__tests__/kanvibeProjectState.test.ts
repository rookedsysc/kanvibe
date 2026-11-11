import { describe, expect, it, vi, beforeEach } from "vitest";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";

const { mockReadTextFile, mockReadTextFiles, mockWriteTextFile, mockWriteTextFileIfAbsent } = vi.hoisted(() => ({
  mockReadTextFile: vi.fn(),
  mockReadTextFiles: vi.fn(),
  mockWriteTextFile: vi.fn(),
  mockWriteTextFileIfAbsent: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  findSymbolicLinkPath: async () => null,
  readTextFile: (...args: unknown[]) => mockReadTextFile(...args),
  readTextFiles: (...args: unknown[]) => mockReadTextFiles(...args),
  updateTextFile: async (targetPath: string, update: (content: string) => string, sshHost?: string | null, mode?: number) => {
    const content = await mockReadTextFile(targetPath, sshHost);
    await mockWriteTextFile(targetPath, update(content), sshHost, ...(mode !== undefined ? [mode] : []));
  },
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
  parseKanvibeProjectState,
  parseKanvibeTaskState,
  parseProjectColor,
  parseTaskStatus,
  readKanvibeProjectColor,
  readKanvibeTaskState,
  readKanvibeTaskSyncState,
  upsertKanvibeHookTarget,
  writeKanvibeProjectColor,
  writeKanvibeProjectColorIfAbsent,
  writeKanvibeProjectPriority,
  writeKanvibeTaskMetadata,
  writeKanvibeTaskSession,
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

  it("프로젝트 priority는 project.json에 저장하고 기존 색상과 함께 읽는다", () => {
    const content = buildKanvibeProjectStateContent("#65D08A", TaskPriority.HIGH);

    expect(JSON.parse(content)).toEqual({
      schemaVersion: 1,
      projectColor: "#65D08A",
      priority: TaskPriority.HIGH,
      updatedAt: expect.any(String),
    });
    expect(parseKanvibeProjectState(content)).toEqual({
      schemaVersion: 1,
      projectColor: "#65D08A",
      priority: TaskPriority.HIGH,
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

  it("project.json의 색상과 priority를 각각 바꿔도 다른 값을 보존한다", async () => {
    mockReadTextFile
      .mockResolvedValueOnce(JSON.stringify({ projectColor: "#65D08A", priority: "low" }))
      .mockResolvedValueOnce(JSON.stringify({ projectColor: "#0064FF", priority: "low" }));

    await writeKanvibeProjectColor("/local/repo", "#0064FF");
    expect(JSON.parse(getLastWrittenContent())).toEqual(expect.objectContaining({
      projectColor: "#0064FF",
      priority: TaskPriority.LOW,
    }));

    await writeKanvibeProjectPriority("/local/repo", TaskPriority.HIGH);
    expect(JSON.parse(getLastWrittenContent())).toEqual(expect.objectContaining({
      projectColor: "#0064FF",
      priority: TaskPriority.HIGH,
    }));
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

  it("전송 종류를 보존하고 알 수 없는 전송 종류를 legacy 등록으로 바꾸지 않는다", () => {
    const targets = [
      { url: "http://localhost:9736", taskId: "local-task", transport: "local" as const },
      { url: "http://10.0.0.5:9736", taskId: "remote-task", clientId: "client-a", transport: "ssh" as const },
    ];
    expect(parseKanvibeTargets(buildKanvibeTargetsContent(targets)).targets).toEqual(targets);
    expect(parseKanvibeTargets(JSON.stringify({ targets: [{ ...targets[0], transport: "invalid" }] })).targets).toEqual([]);
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

  it("다른 client의 CLI 토큰을 지우지 않고 내 토큰을 갱신하며 targets.json을 소유자 전용 권한으로 기록한다", async () => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({
      schemaVersion: 1,
      targets: [
        { url: "http://10.0.0.5:19736", taskId: "peer-task", token: "peer-token" },
        { url: "http://127.0.0.1:19736", taskId: "my-task", token: "old-token" },
      ],
    }));

    await upsertKanvibeHookTarget(
      "/repo",
      { url: "http://127.0.0.1:19736", taskId: "my-task", token: "new-token" },
      null,
    );

    expect(JSON.parse(getLastWrittenContent()).targets).toEqual([
      { url: "http://10.0.0.5:19736", taskId: "peer-task", token: "peer-token" },
      { url: "http://127.0.0.1:19736", taskId: "my-task", token: "new-token" },
    ]);
    expect(mockWriteTextFile).toHaveBeenLastCalledWith("/repo/.kanvibe/targets.json", expect.any(String), null, 0o600);
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

  it.each([
    ["client ID가 같은 항목", { url: "http://192.168.0.10:19736", taskId: "my-task", token: "old-token", clientId: "client-a" }],
    ["client ID가 없던 시절의 같은 task 항목", { url: "http://192.168.0.10:19736", taskId: "my-task", token: "old-token" }],
  ])("IP가 바뀐 뒤 재설치하면 %s을 옛 주소째 교체해 CLI가 옛 주소로 토큰을 보내지 않게 한다", async (_name, staleTarget) => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({
      schemaVersion: 1,
      targets: [
        staleTarget,
        { url: "http://10.0.0.5:19736", taskId: "peer-task", token: "peer-token", clientId: "client-b" },
      ],
    }));

    await upsertKanvibeHookTarget(
      "/remote/repo",
      { url: "http://192.168.0.23:19736", taskId: "my-task", token: "new-token", clientId: "client-a" },
      "ssh-host",
    );

    expect(JSON.parse(getLastWrittenContent()).targets).toEqual([
      { url: "http://192.168.0.23:19736", taskId: "my-task", token: "new-token", clientId: "client-a" },
      { url: "http://10.0.0.5:19736", taskId: "peer-task", token: "peer-token", clientId: "client-b" },
    ]);
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
    await writeKanvibeTaskMetadata("/local/repo", {
      description: "결제 실패 로그 원인 추적",
    });

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

  it("task priority를 task.json에 설명과 함께 기록한다", () => {
    const content = buildKanvibeTaskDescriptionContent(
      "결제 실패 로그 원인 추적",
      TaskPriority.LOW,
    );

    expect(JSON.parse(content)).toEqual({
      schemaVersion: 1,
      description: "결제 실패 로그 원인 추적",
      priority: TaskPriority.LOW,
      updatedAt: expect.any(String),
    });
    expect(parseKanvibeTaskDescription(content)).toEqual({
      schemaVersion: 1,
      description: "결제 실패 로그 원인 추적",
      priority: TaskPriority.LOW,
      updatedAt: expect.any(String),
    });
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

  it("세션 정보를 기록할 때 이미 있는 설명과 priority를 남긴다", async () => {
    mockReadTextFile.mockResolvedValue(buildKanvibeTaskDescriptionContent("결제 로그 추적", TaskPriority.HIGH));

    await writeKanvibeTaskSession("/remote/repo", { sessionType: SessionType.ZELLIJ, sessionName: "repo-feat" }, "build-host");

    expect(mockWriteTextFile).toHaveBeenCalledWith("/remote/repo/.kanvibe/task.json", expect.any(String), "build-host");
    expect(JSON.parse(getLastWrittenContent())).toEqual({
      schemaVersion: 1,
      description: "결제 로그 추적",
      priority: TaskPriority.HIGH,
      session: { sessionType: SessionType.ZELLIJ, sessionName: "repo-feat" },
      updatedAt: expect.any(String),
    });
  });

  it("설명을 기록할 때 이미 있는 세션 정보를 남긴다", async () => {
    mockReadTextFile.mockResolvedValue(buildKanvibeTaskDescriptionContent(
      undefined,
      undefined,
      { sessionType: SessionType.TMUX, sessionName: "repo-feat" },
    ));

    await writeKanvibeTaskMetadata("/local/repo", { description: "새 설명" });

    expect(JSON.parse(getLastWrittenContent())).toEqual({
      schemaVersion: 1,
      description: "새 설명",
      session: { sessionType: SessionType.TMUX, sessionName: "repo-feat" },
      updatedAt: expect.any(String),
    });
  });

  it("세션 정보만 있는 task.json도 읽고, 모르는 세션 타입이나 빈 이름은 기록 없음으로 본다", () => {
    expect(parseKanvibeTaskDescription(JSON.stringify({
      session: { sessionType: "zellij", sessionName: "repo-feat" },
    }))?.session).toEqual({ sessionType: SessionType.ZELLIJ, sessionName: "repo-feat" });
    expect(parseKanvibeTaskDescription(JSON.stringify({
      session: { sessionType: "screen", sessionName: "repo-feat" },
    }))).toBeNull();
    expect(parseKanvibeTaskDescription(JSON.stringify({
      description: "설명",
      session: { sessionType: "tmux", sessionName: "" },
    }))?.session).toBeUndefined();
  });

  it("동기화 상태를 읽을 때 task.json의 세션 정보를 함께 돌려준다", async () => {
    mockReadTextFiles.mockResolvedValue(new Map([
      ["/local/repo/.kanvibe/task.json", {
        exists: true,
        content: JSON.stringify({ session: { sessionType: "herdr", sessionName: "repo-feat" } }),
      }],
    ]));

    const syncState = await readKanvibeTaskSyncState("/local/repo");

    expect(syncState.session).toEqual({ sessionType: SessionType.HERDR, sessionName: "repo-feat" });
    expect(syncState.description).toBeNull();
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
      [
        "/local/repo/.kanvibe/status.json",
        "/local/repo/.kanvibe/task.json",
        "/local/repo/.kanvibe/project.json",
      ],
      undefined,
    );
  });

  it("task priority를 우선하고 없으면 project priority를 사용한다", async () => {
    mockReadTextFiles
      .mockResolvedValueOnce(new Map([
        ["/local/repo/.kanvibe/status.json", { exists: false, content: "" }],
        ["/local/repo/.kanvibe/task.json", { exists: true, content: JSON.stringify({ priority: "low" }) }],
        ["/project/repo/.kanvibe/project.json", { exists: true, content: JSON.stringify({ priority: "high" }) }],
      ]))
      .mockResolvedValueOnce(new Map([
        ["/local/repo/.kanvibe/status.json", { exists: false, content: "" }],
        ["/local/repo/.kanvibe/task.json", { exists: true, content: JSON.stringify({ description: null }) }],
        ["/project/repo/.kanvibe/project.json", { exists: true, content: JSON.stringify({ priority: "high" }) }],
      ]))
      .mockResolvedValueOnce(new Map([
        ["/local/repo/.kanvibe/status.json", { exists: false, content: "" }],
        ["/local/repo/.kanvibe/task.json", { exists: true, content: JSON.stringify({ priority: null }) }],
        ["/project/repo/.kanvibe/project.json", { exists: true, content: JSON.stringify({ priority: "high" }) }],
      ]));

    await expect(readKanvibeTaskSyncState("/local/repo", undefined, "/project/repo"))
      .resolves.toEqual(expect.objectContaining({
        priority: TaskPriority.LOW,
        projectPriority: TaskPriority.HIGH,
      }));
    await expect(readKanvibeTaskSyncState("/local/repo", undefined, "/project/repo"))
      .resolves.toEqual(expect.objectContaining({ projectPriority: TaskPriority.HIGH }));
    await expect(readKanvibeTaskSyncState("/local/repo", undefined, "/project/repo"))
      .resolves.toEqual(expect.objectContaining({
        priority: null,
        projectPriority: TaskPriority.HIGH,
      }));
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
