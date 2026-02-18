import { MigrationInterface, QueryRunner } from "typeorm";

export class ReplaceColorIndexWithColor1771388085809 implements MigrationInterface {
    name = 'ReplaceColorIndexWithColor1771388085809'

    public async up(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`ALTER TABLE "projects" RENAME COLUMN "color_index" TO "color"`);
        await queryRunner.query(`ALTER TABLE "projects" DROP COLUMN "color"`);
        await queryRunner.query(`ALTER TABLE "projects" ADD "color" character varying(7)`);
    }

    public async down(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`ALTER TABLE "projects" DROP COLUMN "color"`);
        await queryRunner.query(`ALTER TABLE "projects" ADD "color" smallint`);
        await queryRunner.query(`ALTER TABLE "projects" RENAME COLUMN "color" TO "color_index"`);
    }

}
