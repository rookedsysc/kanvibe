import { afterAll, beforeAll, describe, expect, it } from "vitest";
import http from "node:http";
import { execFileSync } from "node:child_process";
import { createNotification, getNotificationById } from "@/desktop/main/notificationStore";
import { createRequire } from "node:module";
import { mkdtemp, mkdir, writeFile, readFile, symlink, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { getDataSource, getProjectRepository, getTaskRepository } from "@/lib/database";
import { TaskStatus } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import * as features from "../mobileFeatureService";

const require = createRequire(import.meta.url);
const { handleMobileRequest } = require("../../../../../electron/mobileRoutes.js");
let server: http.Server;
let baseUrl: string;
let projectId: string;
let taskId: string;
let fixtureDirectory: string;

async function call(method: string, pathname: string, body?: unknown, token = "fixture-token") {
  const response = await fetch(`${baseUrl}/api/mobile${pathname}`, { method, headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  return { status: response.status, body: await response.json() as Record<string, any> };
}

beforeAll(async () => {
  if (!process.env.KANVIBE_DB_PATH?.startsWith(path.join(tmpdir(), "kanvibe-vitest-"))) throw new Error("This integration test requires the isolated Vitest database");
  fixtureDirectory = await mkdtemp(path.join(tmpdir(), "kanvibe-mobile-api-"));
  const projectRepo = await getProjectRepository();
  const project = await projectRepo.save(projectRepo.create({ name: "mobile-provider-fixture", repoPath: fixtureDirectory, defaultBranch: "main", isWorktree: false }));
  projectId = project.id;
  const taskRepo = await getTaskRepository();
  const tasks = await taskRepo.save(Array.from({ length: 25 }, (_, index) => taskRepo.create({ title: `fixture done ${index}`, projectId, status: TaskStatus.DONE, branchName: null })));
  taskId = tasks[0].id;
  const bridge = { ...features, authorizeMobileRequest: async (header: string) => header === "Bearer fixture-token" ? "fixture-device" : null };
  server = http.createServer((request, response) => void handleMobileRequest(request, response, { bridge, host: "127.0.0.1", port: 4885 }));
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  baseUrl = `http://127.0.0.1:${(server.address() as import("node:net").AddressInfo).port}`;
});

afterAll(async () => {
  if (server) await new Promise<void>((resolve) => server.close(() => resolve()));
  if (projectId) {
    await (await getTaskRepository()).delete({ projectId });
    await (await getProjectRepository()).delete(projectId);
  }
  if (fixtureDirectory) await rm(fixtureDirectory, { recursive: true, force: true });
  const ds = await getDataSource();
  if (ds.isInitialized) await ds.destroy();
});

describe("mobile feature providers through real authenticated HTTP and isolated SQLite", () => {
  it("returns searchable tasks beyond the initial Done page", async () => {
    const response = await call("GET", "/tasks/search");
    expect(response.status).toBe(200);
    expect(response.body.tasks.filter((task: any) => task.projectId === projectId)).toHaveLength(25);
  });
  it("loads the remaining Done page with the actual total", async () => {
    const first = await call("GET", "/tasks/done?offset=0&limit=20");
    const second = await call("GET", "/tasks/done?offset=20&limit=20");
    expect(first.status).toBe(200);
    expect(first.body.tasks).toHaveLength(20);
    expect(second.body.tasks).toHaveLength(5);
    expect(second.body.doneTotal).toBe(25);
    expect(new Set([...first.body.tasks, ...second.body.tasks].map((task) => task.id)).size).toBe(25);
  });
  it("persists description and nullable priority while preserving task identity", async () => {
    const saved = await call("PATCH", `/tasks/${taskId}/details`, { description: "한글 새 설명", priority: "high" });
    expect(saved.status).toBe(200);
    expect(saved.body.task).toMatchObject({ id: taskId, description: "한글 새 설명", priority: "high" });
    const cleared = await call("PATCH", `/tasks/${taskId}/details`, { priority: null });
    expect(cleared.status).toBe(200);
    expect((await (await getTaskRepository()).findOneByOrFail({ id: taskId })).priority).toBeNull();
  });
  it("rejects unknown fields, invalid priorities, missing tasks and unpaired requests without writes", async () => {
    const before = await (await getTaskRepository()).findOneByOrFail({ id: taskId });
    expect((await call("PATCH", `/tasks/${taskId}/details`, { title: "forbidden" })).status).toBe(400);
    expect((await call("PATCH", `/tasks/${taskId}/details`, { priority: "urgent" })).status).toBe(400);
    expect((await call("PATCH", "/tasks/not-found/details", { priority: TaskPriority.LOW })).status).toBe(404);
    expect((await call("PATCH", `/tasks/${taskId}/details`, { priority: "low" }, "wrong")).status).toBe(401);
    const after = await (await getTaskRepository()).findOneByOrFail({ id: taskId });
    expect(after).toEqual(before);
  });
  it("saves typed PC settings and pane layouts using the shared desktop stores", async () => {
    const initial = await call("GET", "/settings");
    expect(initial.status).toBe(200);
    try {
      const saved = await call("PATCH", "/settings", { defaultSessionType: "zellij", sortPreference: { keys: [{ field: "title", direction: "asc" }] } });
      expect(saved.status).toBe(200);
      expect(saved.body.settings).toMatchObject({ defaultSessionType: "zellij", sortPreference: { keys: [{ field: "title", direction: "asc" }] } });
      const invalid = await call("PATCH", "/settings", { defaultSessionType: "herdr", backgroundSync: { isEnabled: true, intervalMs: 1 } });
      expect(invalid.status).toBe(400);
      expect((await call("GET", "/settings")).body.settings.defaultSessionType).toBe("zellij");
    } finally {
      await call("PATCH", "/settings", { defaultSessionType: initial.body.settings.defaultSessionType, sortPreference: initial.body.settings.sortPreference });
    }
    const savedLayout = await call("PUT", "/pane-layouts", { isGlobal: false, projectId, layoutType: "vertical_2", panes: [{ position: 0, command: "echo first" }, { position: 1, command: "" }] });
    expect(savedLayout.status).toBe(200);
    const layouts = await call("GET", "/pane-layouts");
    expect(layouts.body.layouts).toEqual(expect.arrayContaining([expect.objectContaining({ projectId, layoutType: "vertical_2" })]));
    expect((await call("DELETE", `/pane-layouts/${savedLayout.body.layout.id}`)).status).toBe(200);
  });
  it("marks the selected task notifications read without affecting another task", async () => {
    const target = await createNotification({ taskId, title: "fixture", body: "read target", locale: "ko", dedupeKey: `target-${taskId}` });
    const other = await createNotification({ taskId: "other-task", title: "fixture", body: "keep unread", locale: "ko", dedupeKey: `other-${taskId}` });
    const response = await call("POST", `/tasks/${taskId}/notifications/read`);
    expect(response.body.updatedCount).toBe(1);
    expect((await getNotificationById(target.notification.id))?.isRead).toBe(true);
    expect((await getNotificationById(other.notification.id))?.isRead).toBe(false);
  });
  it("reads and saves real changed Git files and rejects filesystem escapes", async () => {
    const git = (...args: string[]) => execFileSync("git", ["-C", fixtureDirectory, ...args], { stdio: "pipe" });
    git("init", "-b", "main");
    await mkdir(path.join(fixtureDirectory, "src"));
    await writeFile(path.join(fixtureDirectory, "src/file.ts"), "base\n");
    git("add", "src/file.ts");
    git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.hooksPath=/dev/null", "commit", "-m", "fixture base");
    git("switch", "-c", "feat/provider");
    await writeFile(path.join(fixtureDirectory, "src/file.ts"), "changed\n");
    const taskRepo = await getTaskRepository();
    const diffTask = await taskRepo.save(taskRepo.create({ title: "diff fixture", projectId, worktreePath: fixtureDirectory, branchName: "feat/provider", baseBranch: "main", status: TaskStatus.TODO }));
    const files = await call("GET", `/tasks/${diffTask.id}/diff`);
    expect(files.status).toBe(200);
    expect(files.body.files).toEqual(expect.arrayContaining([expect.objectContaining({ path: "src/file.ts", status: "modified" })]));
    const read = await call("GET", `/tasks/${diffTask.id}/files?path=src%2Ffile.ts`);
    expect(read.body).toMatchObject({ success: true, original: "base", content: "changed\n" });
    expect((await call("PUT", `/tasks/${diffTask.id}/files?path=src%2Ffile.ts`, { content: "한글 저장\n" })).status).toBe(200);
    expect(await readFile(path.join(fixtureDirectory, "src/file.ts"), "utf8")).toBe("한글 저장\n");
    expect((await call("PUT", `/tasks/${diffTask.id}/files?path=..%2Foutside`, { content: "bad" })).status).toBe(400);
    await symlink("/etc/passwd", path.join(fixtureDirectory, "src/escape"));
    expect((await call("GET", `/tasks/${diffTask.id}/files?path=src%2Fescape`)).status).toBe(400);
    expect((await call("GET", `/tasks/${diffTask.id}/files?path=.git%2Fconfig`)).status).toBe(400);
  });

});
