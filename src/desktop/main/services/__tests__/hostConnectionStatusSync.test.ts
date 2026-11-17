// @vitest-environment node
import { EventEmitter } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DataSource } from "typeorm";
import { AppSettings } from "@/entities/AppSettings";
import { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";

/** DB, 원격 명령, 호스트 연결처럼 프로세스 밖으로 나가는 경계만 바꾸고 터널·상태 동기화 서비스는 실제 구현을 쓴다. */
const boundary = vi.hoisted(() => ({
  tasks: undefined as unknown,
  projects: undefined as unknown,
  settings: undefined as unknown,
  readRemoteFile: vi.fn(),
  openHostConnection: vi.fn(),
}));
vi.mock("@/lib/database", () => ({
  getTaskRepository: async () => boundary.tasks,
  getProjectRepository: async () => boundary.projects,
  getAppSettingsRepository: async () => boundary.settings,
}));
vi.mock("@/lib/gitOperations", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/gitOperations")>()),
  execGit: boundary.readRemoteFile,
}));
vi.mock("@/lib/remoteHostConnection", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/remoteHostConnection")>()),
  configureRemoteHostTransport: () => undefined,
  getRemoteHostConnection: boundary.openHostConnection,
  getRemoteHostEndpointKey: async () => "endpoint-a",
  getRemoteHostNamespace: () => "qa-sync",
  stopRemoteHostConnections: () => undefined,
}));
vi.mock("@/lib/sshConfig", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/sshConfig")>()),
  parseSSHConfig: async () => [{ host: "host-a", hostname: "10.0.0.7", port: 22, username: "dev" }],
}));
import { startHookTunnels, stopHookTunnels } from "@/desktop/main/services/hookTunnelService";

class FakeHostConnection extends EventEmitter {
  setHookHandler(): void {}
}

describe("host 연결과 재연결 뒤의 태스크 상태", () => {
  let database: DataSource;
  let hostConnections: FakeHostConnection[];

  beforeEach(async () => {
    database = new DataSource({ type: "better-sqlite3", database: ":memory:", entities: [KanbanTask, Project, AppSettings], synchronize: true });
    await database.initialize();
    boundary.tasks = database.getRepository(KanbanTask);
    boundary.projects = database.getRepository(Project);
    boundary.settings = database.getRepository(AppSettings);
    hostConnections = [];
    boundary.readRemoteFile.mockReset();
    boundary.openHostConnection.mockReset();
    boundary.openHostConnection.mockImplementation(async () => {
      const hostConnection = new FakeHostConnection();
      hostConnections.push(hostConnection);
      return hostConnection;
    });
    await database.getRepository(KanbanTask).insert({ id: "fixture", title: "fixture", status: TaskStatus.PROGRESS, sshHost: "host-a", worktreePath: "/fixture" });
  });

  afterEach(async () => {
    stopHookTunnels();
    await database?.destroy();
  });

  const readStoredStatus = async () => (await database.getRepository(KanbanTask).findOneByOrFail({ id: "fixture" })).status;

  it("연결되면 원격 상태 파일의 값을 반영하고, 끊겼다 다시 연결되면 그 사이 바뀐 최신 값을 반영한다", async () => {
    // Given
    boundary.readRemoteFile.mockResolvedValue('{"schemaVersion":1,"status":"review"}');

    // When
    startHookTunnels(() => () => undefined, "qa-sync");

    // Then
    await vi.waitFor(async () => expect(await readStoredStatus()).toBe(TaskStatus.REVIEW));

    // Given
    boundary.readRemoteFile.mockResolvedValue('{"schemaVersion":1,"status":"done"}');

    // When
    hostConnections[0].emit("close");

    // Then
    await vi.waitFor(async () => expect(await readStoredStatus()).toBe(TaskStatus.DONE));
  });
});
