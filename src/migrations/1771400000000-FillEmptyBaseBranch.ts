import { MigrationInterface, QueryRunner } from "typeorm";

export class FillEmptyBaseBranch1771400000000 implements MigrationInterface {
    name = 'FillEmptyBaseBranch1771400000000'

    public async up(queryRunner: QueryRunner): Promise<void> {
        await queryRunner.query(`
            UPDATE kanban_tasks t
            SET base_branch = p.default_branch
            FROM projects p
            WHERE t.project_id = p.id
              AND (t.base_branch IS NULL OR t.base_branch = '')
        `);
    }

    public async down(queryRunner: QueryRunner): Promise<void> {
        /** 보정 전 원본 값을 복원할 수 없으므로 rollback은 no-op으로 처리한다 */
    }

}
