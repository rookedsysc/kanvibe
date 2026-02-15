import { MigrationInterface, QueryRunner } from "typeorm";

export class AddAppSettings1771166907165 implements MigrationInterface {
    name = 'AddAppSettings1771166907165'

    public async up(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`CREATE TABLE "app_settings" ("id" uuid NOT NULL DEFAULT uuid_generate_v4(), "key" character varying(100) NOT NULL, "value" text NOT NULL, "created_at" TIMESTAMP NOT NULL DEFAULT now(), "updated_at" TIMESTAMP NOT NULL DEFAULT now(), CONSTRAINT "UQ_app_settings_key" UNIQUE ("key"), CONSTRAINT "PK_app_settings" PRIMARY KEY ("id"))`);
    }

    public async down(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`DROP TABLE "app_settings"`);
    }

}
