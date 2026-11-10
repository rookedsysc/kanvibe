PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE "app_settings" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "key" TEXT NOT NULL UNIQUE,
        "value" TEXT NOT NULL,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      );
INSERT INTO "app_settings" VALUES('74221a66-f7bb-4e4b-962d-f2f3a38d0881','theme_preference','dark','2026-11-02 13:00:31','2026-11-02 13:00:31');
INSERT INTO "app_settings" VALUES('83e75270-9bee-46cd-89ae-32bdecd5d99a','background_sync_enabled','false','2026-11-02 13:00:31','2026-11-02 13:00:31');
INSERT INTO "app_settings" VALUES('8395d0f9-efbe-48e9-a13c-05d10b2360d7','release_update_dismissed_versions','["999.999.999"]','2026-11-02 13:00:31','2026-11-02 13:00:31');
INSERT INTO "app_settings" VALUES('72f5e68a-b7ea-4495-9763-e541a7fe8a94','ai_usage_snapshot_cache','{"version":1,"snapshot":{"accounts":[{"provider":"claude","accountId":"0cddaa3d7068e644","label":"roky.ai.share@gmail.com","status":"ok","planName":"max","windows":[{"kind":"session","modelName":null,"usedPercent":20,"resetsAt":"2026-11-02T14:20:00.524Z"},{"kind":"weekly","modelName":null,"usedPercent":10,"resetsAt":"2026-11-08T18:00:00.524Z"},{"kind":"weekly","modelName":"Fable","usedPercent":0,"resetsAt":"2026-11-08T18:00:00.000Z"}],"reason":null,"fetchedAt":"2026-11-02T13:00:39.624Z"},{"provider":"codex","accountId":"e52457037a40da57","label":"ji.baek@allenslibrary.com","status":"ok","planName":"prolite","windows":[{"kind":"weekly","modelName":null,"usedPercent":96,"resetsAt":"2026-11-04T06:38:53.000Z"}],"reason":null,"fetchedAt":"2026-11-02T13:00:39.382Z"},{"provider":"gemini","accountId":"48b48768ace6fef8","label":"Gemini","status":"unavailable","planName":null,"windows":[],"reason":"gemini-cli-not-found","fetchedAt":"2026-11-02T13:00:39.091Z"}],"fetchedAt":"2026-11-02T13:00:39.624Z"}}','2026-11-02 13:00:39','2026-11-02 13:00:39');
CREATE TABLE "kanban_tasks" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "title" TEXT NOT NULL,
        "description" TEXT,
        "status" TEXT NOT NULL DEFAULT 'todo',
        "branch_name" TEXT,
        "worktree_path" TEXT,
        "session_type" TEXT,
        "session_name" TEXT,
        "ssh_host" TEXT,
        "agent_type" TEXT,
        "project_id" TEXT,
        "base_branch" TEXT,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, "pr_url" TEXT, "priority" TEXT DEFAULT NULL,
        FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE SET NULL
      );
INSERT INTO "kanban_tasks" VALUES('69732318-f4f3-4512-b0fb-7a9e81844ef4','main',NULL,'todo','main','/fixture/repo','tmux','project-clone-tauri-baseline-main',NULL,NULL,'74c8218c-6a1b-4be4-956e-de295b5a3bc9','main','2026-11-02 13:00:31','2026-11-02 13:00:31',NULL,NULL);
INSERT INTO "kanban_tasks" VALUES('ba57e31e-4ced-4621-a91f-896f86cb3709','Tauri UI parity','React UI / Rust backend','todo',NULL,NULL,NULL,NULL,NULL,NULL,'74c8218c-6a1b-4be4-956e-de295b5a3bc9',NULL,'2026-11-02 13:00:31','2026-11-02 13:00:31',NULL,NULL);
CREATE TABLE "migrations" ("id" integer PRIMARY KEY AUTOINCREMENT NOT NULL, "timestamp" bigint NOT NULL, "name" varchar NOT NULL);
INSERT INTO "migrations" VALUES(1,1770854400000,'InitialSchema1770854400000');
INSERT INTO "migrations" VALUES(2,1770854400001,'AddPrUrlToKanbanTasks1770854400001');
INSERT INTO "migrations" VALUES(3,1770854400002,'AddIsWorktreeToProjects1770854400002');
INSERT INTO "migrations" VALUES(4,1771048256887,'AddPaneLayoutConfig1771048256887');
INSERT INTO "migrations" VALUES(5,1771166346785,'AssignDisplayOrder1771166346785');
INSERT INTO "migrations" VALUES(6,1771166907165,'AddAppSettings1771166907165');
INSERT INTO "migrations" VALUES(7,1771171200000,'AddPendingStatus1771171200000');
INSERT INTO "migrations" VALUES(8,1771257600000,'RemoveBranchNameUnique1771257600000');
INSERT INTO "migrations" VALUES(9,1771343199455,'AddColorIndexToProjects1771343199455');
INSERT INTO "migrations" VALUES(10,1771344000000,'AddPriorityToKanbanTasks1771344000000');
INSERT INTO "migrations" VALUES(11,1771388085809,'ReplaceColorIndexWithColor1771388085809');
INSERT INTO "migrations" VALUES(12,1771400000000,'FillEmptyBaseBranch1771400000000');
INSERT INTO "migrations" VALUES(13,1771500000000,'AddIconDataUrlToProjects1771500000000');
INSERT INTO "migrations" VALUES(14,1771600000000,'RescopeProjectNamesPerHost1771600000000');
INSERT INTO "migrations" VALUES(15,1771700000000,'AddTerminalSessionType1771700000000');
INSERT INTO "migrations" VALUES(16,1771800000000,'DropDisplayOrderFromKanbanTasks1771800000000');
INSERT INTO "migrations" VALUES(17,1771900000000,'AddTaskDiffStats1771900000000');
CREATE TABLE "pane_layout_configs" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "layout_type" TEXT NOT NULL,
        "panes" TEXT NOT NULL,
        "project_id" TEXT UNIQUE,
        "is_global" INTEGER NOT NULL DEFAULT 0,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE CASCADE
      );
CREATE TABLE "projects" (
      id TEXT PRIMARY KEY NOT NULL,
      name TEXT NOT NULL,
      repo_path TEXT NOT NULL,
      default_branch TEXT NOT NULL DEFAULT 'main',
      ssh_host TEXT,
      is_worktree INTEGER NOT NULL DEFAULT 0,
      color TEXT DEFAULT NULL,
      icon_data_url TEXT DEFAULT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
INSERT INTO "projects" VALUES('74c8218c-6a1b-4be4-956e-de295b5a3bc9','Tauri migration QA','/fixture/repo','main',NULL,0,'#C4B5FD','data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=','2026-11-02 13:00:31');
DELETE FROM "sqlite_sequence";
INSERT INTO "sqlite_sequence" VALUES('migrations',17);
CREATE TABLE "task_diff_stats" (
        "task_id" TEXT PRIMARY KEY NOT NULL,
        "file_count" INTEGER NOT NULL DEFAULT 0,
        "additions" INTEGER NOT NULL DEFAULT 0,
        "deletions" INTEGER NOT NULL DEFAULT 0,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("task_id") REFERENCES "kanban_tasks"("id") ON DELETE CASCADE
      );
CREATE INDEX "idx_kanban_tasks_project_branch"
        ON kanban_tasks(project_id, branch_name)
    ;
CREATE INDEX "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, updated_at)
    ;
COMMIT;
