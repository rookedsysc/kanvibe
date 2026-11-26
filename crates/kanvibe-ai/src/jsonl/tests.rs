use std::io::Write;
use std::sync::Arc;

use serde_json::{Value, json};

use super::*;
use crate::test_support::{ScriptedRunner, remote_host, temp_home};

fn line(value: Value) -> String {
    format!("{}\n", crate::javascript::stringify(&value))
}

fn append(file_path: &str, content: &str) {
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(file_path)
        .unwrap();
    file.write_all(content.as_bytes()).unwrap();
}

fn values(lines: &AppendedJsonLines) -> Vec<Value> {
    lines
        .lines
        .iter()
        .map(|line| line.as_ref().clone())
        .collect()
}

fn session_file() -> (tempfile::TempDir, String) {
    let (directory, root) = temp_home();
    (directory, format!("{root}/session.jsonl"))
}

#[test]
fn unchanged_file_returns_same_lines() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, line(json!({"id": 1})) + &line(json!({"id": 2}))).unwrap();
    let first = read_appended_json_lines(&file_path).unwrap();
    let second = read_appended_json_lines(&file_path).unwrap();
    assert_eq!(second.log_id, first.log_id);
    assert!(Arc::ptr_eq(&second.lines[0], &first.lines[0]));
    assert_eq!(values(&second), [json!({"id": 1}), json!({"id": 2})]);
}

#[test]
fn appends_only_new_lines_to_the_same_log() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, line(json!({"id": 1}))).unwrap();
    let first = read_appended_json_lines(&file_path).unwrap();
    append(
        &file_path,
        &(line(json!({"id": 2})) + &line(json!({"id": 3}))),
    );
    let second = read_appended_json_lines(&file_path).unwrap();
    assert_eq!(second.log_id, first.log_id);
    assert!(Arc::ptr_eq(&second.lines[0], &first.lines[0]));
    assert_eq!(
        values(&second),
        [json!({"id": 1}), json!({"id": 2}), json!({"id": 3})]
    );
}

#[test]
fn half_written_last_line_waits_until_complete() {
    let (_directory, file_path) = session_file();
    std::fs::write(
        &file_path,
        format!("{}{{\"id\":2,\"text\":\"hal", line(json!({"id": 1}))),
    )
    .unwrap();
    assert_eq!(
        values(&read_appended_json_lines(&file_path).unwrap()),
        [json!({"id": 1})]
    );
    append(&file_path, "f\"}\n");
    assert_eq!(
        values(&read_appended_json_lines(&file_path).unwrap()),
        [json!({"id": 1}), json!({"id": 2, "text": "half"})]
    );
    assert_eq!(read_appended_json_lines(&file_path).unwrap().lines.len(), 2);
}

#[test]
fn complete_last_line_without_newline_is_read_once() {
    let (_directory, file_path) = session_file();
    std::fs::write(
        &file_path,
        format!("{}{}", line(json!({"id": 1})), json!({"id": 2})),
    )
    .unwrap();
    assert_eq!(
        values(&read_appended_json_lines(&file_path).unwrap()),
        [json!({"id": 1}), json!({"id": 2})]
    );
    append(&file_path, &format!("\n{}", line(json!({"id": 3}))));
    assert_eq!(
        values(&read_appended_json_lines(&file_path).unwrap()),
        [json!({"id": 1}), json!({"id": 2}), json!({"id": 3})]
    );
}

#[test]
fn skips_blank_and_unparsable_lines() {
    let (_directory, file_path) = session_file();
    std::fs::write(
        &file_path,
        format!(
            "{}\nnot json\n{}",
            line(json!({"id": 1})),
            line(json!({"id": 2}))
        ),
    )
    .unwrap();
    assert_eq!(
        values(&read_appended_json_lines(&file_path).unwrap()),
        [json!({"id": 1}), json!({"id": 2})]
    );
}

#[test]
fn resumes_at_exact_byte_after_multibyte_text() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, line(json!({"text": "한글 기록 🙂"}))).unwrap();
    read_appended_json_lines(&file_path).unwrap();
    append(&file_path, &line(json!({"text": "다음 줄"})));
    assert_eq!(
        values(&read_appended_json_lines(&file_path).unwrap()),
        [json!({"text": "한글 기록 🙂"}), json!({"text": "다음 줄"})]
    );
}

#[test]
fn shrunk_file_is_reread_into_a_new_log() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, line(json!({"id": 1})) + &line(json!({"id": 2}))).unwrap();
    let first = read_appended_json_lines(&file_path).unwrap();
    std::fs::write(&file_path, line(json!({"id": 9}))).unwrap();
    let second = read_appended_json_lines(&file_path).unwrap();
    assert_ne!(second.log_id, first.log_id);
    assert_eq!(values(&second), [json!({"id": 9})]);
}

#[test]
fn rewritten_longer_content_is_reread() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, line(json!({"id": 1}))).unwrap();
    let first = read_appended_json_lines(&file_path).unwrap();
    std::fs::write(&file_path, line(json!({"id": 7})) + &line(json!({"id": 8}))).unwrap();
    let second = read_appended_json_lines(&file_path).unwrap();
    assert_ne!(second.log_id, first.log_id);
    assert_eq!(values(&second), [json!({"id": 7}), json!({"id": 8})]);
}

#[test]
fn replaced_file_is_reread() {
    let (directory, file_path) = session_file();
    std::fs::write(&file_path, line(json!({"id": 1}))).unwrap();
    let first = read_appended_json_lines(&file_path).unwrap();
    let replacement = directory.path().join("replacement.jsonl");
    std::fs::write(
        &replacement,
        line(json!({"id": 1})) + &line(json!({"id": 5})),
    )
    .unwrap();
    std::fs::rename(&replacement, &file_path).unwrap();
    let second = read_appended_json_lines(&file_path).unwrap();
    assert_ne!(second.log_id, first.log_id);
    assert_eq!(values(&second), [json!({"id": 1}), json!({"id": 5})]);
}

#[test]
fn concurrent_reads_do_not_duplicate_appended_lines() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, line(json!({"id": 1}))).unwrap();
    read_appended_json_lines(&file_path).unwrap();
    append(&file_path, &line(json!({"id": 2})));
    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(|| read_appended_json_lines(&file_path).unwrap());
        let right = scope.spawn(|| read_appended_json_lines(&file_path).unwrap());
        (left.join().unwrap(), right.join().unwrap())
    });
    assert_eq!(left.log_id, right.log_id);
    assert_eq!(values(&left), [json!({"id": 1}), json!({"id": 2})]);
    assert_eq!(values(&right), [json!({"id": 1}), json!({"id": 2})]);
}

#[test]
fn missing_file_is_a_read_error() {
    let (_directory, root) = temp_home();
    let error = read_appended_json_lines(&format!("{root}/missing.jsonl")).unwrap_err();
    assert!(error.is_not_found());
}

#[test]
fn remote_head_reads_requested_lines_with_quoted_path() {
    let runner = ScriptedRunner::new(|_, _| Ok("{\"source\":\"head\"}\n".to_string()));
    let host = remote_host(runner.clone(), "remote");
    assert_eq!(
        read_json_lines_head(&host, "/tmp/a'b.jsonl", 1).unwrap(),
        [json!({"source": "head"})]
    );
    assert_eq!(runner.commands(), ["head -n 1 '/tmp/a'\\''b.jsonl'"]);
    assert_eq!(
        runner.calls.lock().unwrap()[0].ssh_host.as_deref(),
        Some("remote")
    );
}

#[test]
fn local_head_stops_after_requested_values_and_splits_carriage_returns() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, "{\"a\":1}\r\n\r{\"b\":2}\rbroken\n{\"c\":3}\n").unwrap();
    let host = crate::test_support::local_host("/unused");
    assert_eq!(
        read_json_lines_head(&host, &file_path, 2).unwrap(),
        [json!({"a": 1}), json!({"b": 2})]
    );
    assert_eq!(read_json_lines_head(&host, &file_path, 5).unwrap().len(), 3);
}

#[test]
fn local_tail_keeps_last_values_and_drops_null_lines() {
    let (_directory, file_path) = session_file();
    std::fs::write(&file_path, "{\"a\":1}\nnull\n{\"b\":2}\n{\"c\":3}\n").unwrap();
    let host = crate::test_support::local_host("/unused");
    assert_eq!(
        read_json_lines_tail(&host, &file_path, 2).unwrap(),
        [json!({"b": 2}), json!({"c": 3})]
    );
    assert_eq!(read_json_lines(&host, &file_path).unwrap().len(), 3);
}

#[test]
fn appending_while_the_previous_result_is_held_keeps_the_earlier_lines_shared() {
    let (_directory, file_path) = session_file();
    let earlier_lines: String = (0..1000).map(|id| line(json!({ "id": id }))).collect();
    std::fs::write(&file_path, earlier_lines).unwrap();
    let first = read_appended_json_lines(&file_path).unwrap();

    append(&file_path, &line(json!({"id": 1000})));
    let second = read_appended_json_lines(&file_path).unwrap();

    assert!(Arc::ptr_eq(&second.lines.chunks[0], &first.lines.chunks[0]));
    assert_eq!(second.lines.len(), 1001);
    assert_eq!(second.lines[1000].as_ref(), &json!({"id": 1000}));
}

#[test]
fn chunked_lines_index_like_one_list_and_stay_few_chunks() {
    let mut chunks = JsonLineChunks::default();
    let mut expected = Vec::new();
    for (round, size) in [1, 3, 2, 2, 7, 1, 1, 1, 16, 5, 0, 4]
        .into_iter()
        .enumerate()
    {
        let appended: Vec<_> = (0..size)
            .map(|offset| Arc::new(json!({ "round": round, "offset": offset })))
            .collect();
        expected.extend(appended.iter().cloned());
        chunks.append(appended);
    }

    assert_eq!(chunks.len(), expected.len());
    for (index, value) in expected.iter().enumerate() {
        assert!(Arc::ptr_eq(&chunks[index], value), "line {index}");
    }
    assert!(
        chunks
            .iter()
            .zip(&expected)
            .all(|(left, right)| Arc::ptr_eq(left, right))
    );
    assert!(chunks.chunks.len() <= expected.len().ilog2() as usize + 1);
}
