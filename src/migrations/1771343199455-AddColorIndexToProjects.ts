import { MigrationInterface, QueryRunner } from "typeorm";

export class AddColorIndexToProjects1771343199455 implements MigrationInterface {
    name = 'AddColorIndexToProjects1771343199455'

    public async up(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`ALTER TABLE "projects" ADD "color_index" smallint`);
    }

    public async down(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`ALTER TABLE "projects" DROP COLUMN "color_index"`);
    }

}
