//! OpenCode SQLite 질의 원문. 원격 Python 스크립트에도 그대로 실려 가므로 TS 템플릿 문자열과 글자 하나까지 같게 둔다.

pub const SQLITE_QUERY_SCRIPT: &str = r#"import base64
import json
import sqlite3
import sys

payload = json.loads(base64.b64decode(sys.argv[1]).decode("utf-8"))
conn = sqlite3.connect(payload["dbPath"])
try:
    conn.row_factory = sqlite3.Row
    rows = conn.execute(payload["sql"], payload.get("parameters") or {}).fetchall()
    print(json.dumps([dict(row) for row in rows], ensure_ascii=False))
finally:
    conn.close()"#;

pub const KNOWN_SESSION_IDS_SQL: &str = "SELECT s.id FROM session s
        WHERE (@worktreePath != '' AND (s.directory = @worktreePath OR instr(s.directory, @worktreePrefix) = 1));";

const SESSION_LIST_HEAD: &str = "SELECT
        s.id,
        s.directory,
        s.title,
        s.time_created,
        s.time_updated,
        (
          SELECT MAX(p.time_created)
          FROM part p
          JOIN message m ON m.id = p.message_id
          WHERE p.session_id = s.id
            AND json_extract(m.data, '$.role') IN ('user', 'assistant')
            AND json_extract(p.data, '$.type') = 'text'
        ) as last_message_at,
        (SELECT COUNT(*) FROM part p WHERE p.session_id = s.id) as part_count,
        (
          SELECT p.data
          FROM part p
          JOIN message m ON m.id = p.message_id
          WHERE p.session_id = s.id
            AND json_extract(m.data, '$.role') = 'user'
            AND json_extract(p.data, '$.type') = 'text'
          ORDER BY p.time_created ASC
          LIMIT 1
        ) as first_user_part,
        ";

const MATCHING_PART_COUNT_SQL: &str = r"(SELECT COUNT(*) FROM part p WHERE p.session_id = s.id AND lower(p.data) LIKE '%' || @query || '%' ESCAPE '\')";

const SESSION_LIST_SCOPE: &str = " as matching_part_count
      FROM session s
      WHERE (@worktreePath != '' AND (s.directory = @worktreePath OR instr(s.directory, @worktreePrefix) = 1))
        ";

const QUERY_FILTER_SQL: &str = r"AND (
          lower(COALESCE(s.title, '')) LIKE '%' || @query || '%' ESCAPE '\'
          OR lower(s.id) LIKE '%' || @query || '%' ESCAPE '\'
          OR lower(s.directory) LIKE '%' || @query || '%' ESCAPE '\'
          OR EXISTS (SELECT 1 FROM part p WHERE p.session_id = s.id AND lower(p.data) LIKE '%' || @query || '%' ESCAPE '\')
        )";

const SESSION_LIST_TAIL: &str = "
      ORDER BY COALESCE(last_message_at, s.time_updated) DESC
      LIMIT 120;";

/// 목록 질의. 검색어가 있으면 본문 일치 수와 검색 조건을 넣는다
pub fn session_list_sql(has_query: bool) -> String {
    let (matching, filter) = if has_query {
        (MATCHING_PART_COUNT_SQL, QUERY_FILTER_SQL)
    } else {
        ("0", "")
    };
    format!("{SESSION_LIST_HEAD}{matching}{SESSION_LIST_SCOPE}{filter}{SESSION_LIST_TAIL}")
}

pub const LIVE_SESSIONS_SQL: &str = "SELECT s.id, s.directory, s.title, s.time_created, s.time_updated
      FROM session s
      WHERE (@worktreePath != '' AND (s.directory = @worktreePath OR instr(s.directory, @worktreePrefix) = 1))
      ORDER BY s.time_updated DESC
      LIMIT 120;";

pub const CHILD_SESSIONS_SQL: &str = "SELECT s.id, s.title, s.time_updated
        FROM session s
        WHERE s.parent_id = @parentSessionId
        ORDER BY s.time_updated DESC;";

pub const DETAIL_ROWS_SQL: &str = "SELECT
      s.id AS session_id,
      s.directory,
      s.title,
      p.message_id,
      p.data AS part_data,
      p.time_created,
      m.data AS message_data
    FROM session s
    JOIN part p ON p.session_id = s.id
    JOIN message m ON m.id = p.message_id
    WHERE s.id = @sessionId
    ORDER BY p.time_created ASC;";

pub const SESSION_ROW_SQL: &str = "SELECT id, directory, title, time_created, time_updated FROM session WHERE id = @sessionId LIMIT 1;";

pub const DESCENDANT_SESSIONS_SQL: &str =
    "WITH RECURSIVE descendant(id, parent_id, title, time_created, time_updated, depth) AS (
          SELECT s.id, s.parent_id, s.title, s.time_created, s.time_updated, 0
            FROM session s
            WHERE s.parent_id = @rootSessionId
          UNION ALL
          SELECT s.id, s.parent_id, s.title, s.time_created, s.time_updated, descendant.depth + 1
            FROM session s
            JOIN descendant ON s.parent_id = descendant.id
            WHERE descendant.depth < 10
        )
        SELECT id, parent_id, title, time_created, time_updated
          FROM descendant
          ORDER BY time_created;";

#[cfg(test)]
mod tests {
    use super::*;

    /// TS 템플릿을 평가한 원문과 한 번 바이트 단위로 대조했고, 여기서는 그 모양의 경계만 고정한다
    #[test]
    fn list_sql_keeps_ts_template_layout() {
        let with_query = session_list_sql(true);
        assert!(
            with_query.contains("ESCAPE '\\')\n      FROM")
                || with_query.contains("ESCAPE '\\') as matching_part_count")
        );
        assert!(session_list_sql(false).contains("        0 as matching_part_count\n"));
        assert!(
            session_list_sql(false)
                .contains("instr(s.directory, @worktreePrefix) = 1))\n        \n      ORDER BY")
        );
    }
}
