import { MigrationInterface, QueryRunner } from "typeorm";

export class AddPaneLayoutConfig1771048256887 implements MigrationInterface {
    name = 'AddPaneLayoutConfig1771048256887'

    public async up(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`ALTER TABLE "kanban_tasks" DROP CONSTRAINT "FK_kanban_tasks_project_id"`);
        await queryRunner.query(`CREATE TABLE "pane_layout_configs" ("id" uuid NOT NULL DEFAULT uuid_generate_v4(), "layout_type" character varying(50) NOT NULL, "panes" jsonb NOT NULL, "project_id" uuid, "is_global" boolean NOT NULL DEFAULT false, "created_at" TIMESTAMP NOT NULL DEFAULT now(), "updated_at" TIMESTAMP NOT NULL DEFAULT now(), CONSTRAINT "UQ_ea7fde0b1049757ae373fa03327" UNIQUE ("project_id"), CONSTRAINT "PK_464ff56e7e259c3f0e6a20d709d" PRIMARY KEY ("id"))`);
        await queryRunner.query(`ALTER TABLE "kanban_tasks" ADD CONSTRAINT "FK_4a4cd49cc92129ef4c53c38c5aa" FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE SET NULL ON UPDATE NO ACTION`);
        await queryRunner.query(`ALTER TABLE "pane_layout_configs" ADD CONSTRAINT "FK_ea7fde0b1049757ae373fa03327" FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE CASCADE ON UPDATE NO ACTION`);
    }

    public async down(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`ALTER TABLE "pane_layout_configs" DROP CONSTRAINT "FK_ea7fde0b1049757ae373fa03327"`);
        await queryRunner.query(`ALTER TABLE "kanban_tasks" DROP CONSTRAINT "FK_4a4cd49cc92129ef4c53c38c5aa"`);
        await queryRunner.query(`DROP TABLE "pane_layout_configs"`);
        await queryRunner.query(`ALTER TABLE "kanban_tasks" ADD CONSTRAINT "FK_kanban_tasks_project_id" FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE SET NULL ON UPDATE NO ACTION`);
    }

}
