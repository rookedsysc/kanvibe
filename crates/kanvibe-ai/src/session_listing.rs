//! 리더 결과 조립, 시각 정렬, 페이지 나누기, worktree 일치 판정, 동시성 제한처럼 provider 리더가 함께 쓰는 목록 처리.

use std::cmp::Ordering;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use crate::javascript::parse_date_ms;
use crate::node_path;
use crate::providers::AiSessionProvider;
use crate::types::{
    AggregatedAiMessage, AggregatedAiSession, AiSessionMatchScope, AiSessionReaderContext,
    AiSessionReaderResult, AiSessionSourceStatus,
};

const MAX_PREVIEW_MESSAGES: usize = 12;
pub const REMOTE_SESSION_FILE_PARSE_CONCURRENCY: usize = 3;
pub const LOCAL_SESSION_FILE_PARSE_CONCURRENCY: usize = 8;

/// 세션을 찾은 리더 결과. 세션 수는 따로 주지 않으면 세션 개수다
pub fn create_reader_result(
    provider: AiSessionProvider,
    sessions: Vec<AggregatedAiSession>,
) -> AiSessionReaderResult {
    AiSessionReaderResult {
        provider,
        available: true,
        session_count: sessions.len() as u64,
        reason: None,
        sessions,
    }
}

/// 원천이 없어 읽지 못한 리더 결과
pub fn create_unavailable_result(
    provider: AiSessionProvider,
    reason: impl Into<String>,
) -> AiSessionReaderResult {
    AiSessionReaderResult {
        provider,
        available: false,
        session_count: 0,
        reason: Some(reason.into()),
        sessions: Vec::new(),
    }
}

pub fn to_source_status(result: &AiSessionReaderResult) -> AiSessionSourceStatus {
    AiSessionSourceStatus {
        provider: result.provider,
        available: result.available,
        session_count: result.session_count,
        reason: result.reason.clone(),
    }
}

/// 수정 시각이 가장 늦은 파일 하나. 같은 시각이면 앞의 것을 남긴다
pub fn pick_latest_by<T>(files: &[T], mtime_ms: impl Fn(&T) -> f64) -> Option<&T> {
    files
        .iter()
        .fold(None, |latest: Option<&T>, file| match latest {
            Some(current) if mtime_ms(file) <= mtime_ms(current) => Some(current),
            _ => Some(file),
        })
}

/// 실행중으로 볼 세션 후보. 창 안에서 움직인 기록은 전부 최근 순으로, 창 안에 없으면 가장 최근 하나만 남겨
/// 유휴 세션도 화면에서 사라지지 않게 한다
pub fn pick_live_session_files<T: Clone>(
    files: &[T],
    running_window_ms: f64,
    now_ms: f64,
    mtime_ms: impl Fn(&T) -> f64,
) -> Vec<T> {
    let running_since = now_ms - running_window_ms;
    let mut running_files: Vec<T> = files
        .iter()
        .filter(|file| mtime_ms(file) >= running_since)
        .cloned()
        .collect();
    if !running_files.is_empty() {
        running_files.sort_by(|left, right| compare_descending(mtime_ms(left), mtime_ms(right)));
        return running_files;
    }
    pick_latest_by(files, mtime_ms)
        .cloned()
        .into_iter()
        .collect()
}

fn compare_descending(left: f64, right: f64) -> Ordering {
    right.partial_cmp(&left).unwrap_or(Ordering::Equal)
}

/// 해석할 수 없는 시각은 뒤로 보내고 나머지는 최근 순으로 둔다. JS 정렬 비교 함수와 같은 결과를 내는 안정 정렬이다
fn compare_optional_times_descending(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => compare_descending(left, right),
    }
}

fn parse_optional_date(value: Option<&str>) -> Option<f64> {
    parse_date_ms(value.unwrap_or_default())
}

pub fn sort_sessions_descending(sessions: &mut [AggregatedAiSession]) {
    sessions.sort_by(|left, right| {
        let left_value =
            parse_optional_date(left.updated_at.as_deref().or(left.started_at.as_deref()));
        let right_value =
            parse_optional_date(right.updated_at.as_deref().or(right.started_at.as_deref()));
        compare_optional_times_descending(left_value, right_value)
    });
}

/// 같은 시각에도 나중에 추가된 기록을 먼저 돌려줘야 화면에서 뒤집을 때 원래 대화 순서가 된다
pub fn sort_messages_descending(messages: &[AggregatedAiMessage]) -> Vec<AggregatedAiMessage> {
    let mut sorted: Vec<AggregatedAiMessage> = messages.iter().rev().cloned().collect();
    sorted.sort_by(|left, right| {
        compare_optional_times_descending(
            parse_optional_date(left.timestamp.as_deref()),
            parse_optional_date(right.timestamp.as_deref()),
        )
    });
    sorted
}

pub fn limit_preview_messages(mut messages: Vec<AggregatedAiMessage>) -> Vec<AggregatedAiMessage> {
    messages.truncate(MAX_PREVIEW_MESSAGES);
    messages
}

/// 기록의 작업 경로가 이 Task의 worktree이거나 그 아래면 worktree 일치로 본다
pub fn determine_match_scope(
    candidate_path: Option<&str>,
    context: &AiSessionReaderContext,
) -> Option<AiSessionMatchScope> {
    let candidate_path = candidate_path.filter(|path| !path.is_empty())?;
    let normalized_candidate = node_path::resolve(&[candidate_path]);
    let normalized_worktree = context
        .worktree_path
        .as_deref()
        .filter(|path| !path.is_empty())
        .map(|path| node_path::resolve(&[path]))?;
    is_path_match(&normalized_candidate, &normalized_worktree)
        .then_some(AiSessionMatchScope::Worktree)
}

pub fn get_candidate_paths(context: &AiSessionReaderContext) -> Vec<String> {
    context
        .worktree_path
        .iter()
        .filter(|path| !path.is_empty())
        .cloned()
        .collect()
}

fn is_path_match(candidate_path: &str, target_path: &str) -> bool {
    candidate_path == target_path
        || candidate_path.starts_with(&format!("{target_path}{}", node_path::SEPARATOR))
}

/// 한 페이지와 다음 페이지 커서. 커서는 시작 위치를 10진 문자열로 담는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

pub fn paginate_items<T: Clone>(items: &[T], cursor: Option<&str>, limit: usize) -> Page<T> {
    let offset = cursor
        .filter(|cursor| !cursor.is_empty())
        .and_then(crate::host::parse_js_int)
        .unwrap_or(0.0);
    let safe_offset = clamp_slice_index(offset, items.len());
    let end = safe_offset.saturating_add(limit).min(items.len());
    let page_items = items[safe_offset..end].to_vec();
    let next_offset = safe_offset + page_items.len();
    Page {
        next_cursor: (next_offset < items.len()).then(|| next_offset.to_string()),
        items: page_items,
    }
}

/// JS `slice`의 시작 위치. 음수는 끝에서 센다
fn clamp_slice_index(offset: f64, length: usize) -> usize {
    if offset < 0.0 {
        return (length as f64 + offset).max(0.0) as usize;
    }
    (offset as usize).min(length)
}

/// 결과 순서를 지키면서 최대 `concurrency`개만 동시에 돌린다
pub fn map_with_concurrency<T, R, F>(items: &[T], concurrency: usize, mapper: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T, usize) -> R + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }
    let worker_count = concurrency.max(1).min(items.len());
    if worker_count == 1 {
        return items
            .iter()
            .enumerate()
            .map(|(index, item)| mapper(item, index))
            .collect();
    }
    let next_index = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let current_index = next_index.fetch_add(1, AtomicOrdering::SeqCst);
                    let Some(item) = items.get(current_index) else {
                        break;
                    };
                    let result = mapper(item, current_index);
                    results.lock().expect("결과 잠금")[current_index] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("결과 잠금")
        .into_iter()
        .map(|result| result.expect("모든 항목을 처리했다"))
        .collect()
}

/// 호스트에 맞는 파싱 동시성
pub fn parse_concurrency(context: &AiSessionReaderContext) -> usize {
    if context.host.is_remote() {
        REMOTE_SESSION_FILE_PARSE_CONCURRENCY
    } else {
        LOCAL_SESSION_FILE_PARSE_CONCURRENCY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AiMessageRole;
    use std::time::Duration;

    fn message(role: AiMessageRole, timestamp: Option<&str>, text: &str) -> AggregatedAiMessage {
        AggregatedAiMessage::plain(role, timestamp, text)
    }

    fn texts(messages: &[AggregatedAiMessage]) -> Vec<&str> {
        messages
            .iter()
            .map(|message| message.text.as_str())
            .collect()
    }

    #[test]
    fn sorts_detail_messages_from_newest_to_oldest() {
        let result = sort_messages_descending(&[
            message(AiMessageRole::User, Some("2026-03-13T09:00:00.000Z"), "old"),
            message(
                AiMessageRole::Assistant,
                Some("2026-03-13T09:02:00.000Z"),
                "new",
            ),
            message(AiMessageRole::Tool, Some("2026-03-13T09:01:00.000Z"), "mid"),
        ]);
        assert_eq!(texts(&result), ["new", "mid", "old"]);
    }

    #[test]
    fn same_instant_keeps_append_order_when_reversed() {
        let messages = [
            message(
                AiMessageRole::User,
                Some("2026-11-12T10:00:00.000Z"),
                "질문",
            ),
            message(
                AiMessageRole::Assistant,
                Some("2026-11-12T19:00:00.000+09:00"),
                "응답",
            ),
            message(
                AiMessageRole::User,
                Some("2026-11-12T10:01:00.000Z"),
                "후속 질문",
            ),
        ];
        let result = sort_messages_descending(&messages);
        assert_eq!(texts(&result), ["후속 질문", "응답", "질문"]);
        let reversed: Vec<_> = result.iter().rev().cloned().collect();
        assert_eq!(texts(&reversed), ["질문", "응답", "후속 질문"]);
        assert_eq!(texts(&messages), ["질문", "응답", "후속 질문"]);
    }

    #[test]
    fn messages_without_time_keep_reverse_append_order() {
        let messages = [
            message(AiMessageRole::User, None, "질문"),
            message(AiMessageRole::Assistant, None, "응답"),
        ];
        assert_eq!(
            texts(&sort_messages_descending(&messages)),
            ["응답", "질문"]
        );
    }

    #[test]
    fn map_with_concurrency_keeps_order_and_limits_workers() {
        let active_count = AtomicUsize::new(0);
        let max_active_count = AtomicUsize::new(0);
        let result = map_with_concurrency(&[1, 2, 3, 4, 5], 2, |value, index| {
            let active = active_count.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            max_active_count.fetch_max(active, AtomicOrdering::SeqCst);
            std::thread::sleep(Duration::from_millis(((5 - index) * 2) as u64));
            active_count.fetch_sub(1, AtomicOrdering::SeqCst);
            value * 10
        });
        assert_eq!(result, [10, 20, 30, 40, 50]);
        assert!(max_active_count.load(AtomicOrdering::SeqCst) <= 2);
    }

    #[test]
    fn paginate_uses_decimal_offsets() {
        let items = [1, 2, 3, 4, 5];
        let first = paginate_items(&items, None, 2);
        assert_eq!(first.items, [1, 2]);
        assert_eq!(first.next_cursor.as_deref(), Some("2"));
        let last = paginate_items(&items, Some("4"), 2);
        assert_eq!(last.items, [5]);
        assert_eq!(last.next_cursor, None);
        assert_eq!(paginate_items(&items, Some("abc"), 1).items, [1]);
    }
}
