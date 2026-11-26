//! 파일 파싱 결과 캐시. 수정 시각이 그대로면 파일을 다시 읽지 않는다.
//!
//! 전체·앞부분·끝부분 파싱 결과는 서로 섞이지 않도록 캐시를 따로 두고, 호스트와 경로를 함께 키로 쓴다.
//! 원격 stat은 초 단위라 같은 수정 시각을 1초 뒤 한 번 더 읽어 마지막 기록을 확정한다.

use std::any::Any;
use std::sync::{Arc, LazyLock, Mutex};

use crate::error::AiResult;
use crate::host::SessionHost;
use crate::javascript::{now_ms, quote_json_string};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseCacheKind {
    /// 파일 전체를 파싱한 결과
    Full,
    /// 앞 N줄만 파싱한 결과
    Head,
    /// 끝 N줄만 파싱한 결과
    Tail,
}

const FILE_PARSE_CACHE_MAX: usize = 200;
const REMOTE_FILE_PARSE_CACHE_TTL_MS: f64 = 1_000.0;

type CachedValue = Arc<dyn Any + Send + Sync>;

#[derive(Clone)]
struct CacheEntry {
    mtime: f64,
    parsed_at: f64,
    is_mtime_settled: bool,
    value: CachedValue,
}

/// 넣은 순서를 지키는 작은 캐시. 꺼내 쓴 항목은 맨 뒤로 옮기고, 가득 차면 맨 앞을 버린다
#[derive(Default)]
struct OrderedCache {
    entries: Vec<(String, CacheEntry)>,
}

impl OrderedCache {
    fn get(&self, key: &str) -> Option<&CacheEntry> {
        self.entries
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, entry)| entry)
    }

    fn remove(&mut self, key: &str) -> Option<CacheEntry> {
        let index = self.entries.iter().position(|(known, _)| known == key)?;
        Some(self.entries.remove(index).1)
    }

    fn touch(&mut self, key: &str) {
        if let Some(entry) = self.remove(key) {
            self.entries.push((key.to_string(), entry));
        }
    }

    /// JS `Map.set`처럼 이미 있는 키는 자리를 지킨 채 값만 바꾼다
    fn insert(&mut self, key: String, entry: CacheEntry) {
        if self.entries.len() >= FILE_PARSE_CACHE_MAX {
            self.entries.remove(0);
        }
        match self.entries.iter_mut().find(|(known, _)| *known == key) {
            Some((_, existing)) => *existing = entry,
            None => self.entries.push((key, entry)),
        }
    }
}

#[derive(Default)]
struct ParseCaches {
    full: OrderedCache,
    head: OrderedCache,
    tail: OrderedCache,
}

impl ParseCaches {
    fn select(&mut self, kind: ParseCacheKind) -> &mut OrderedCache {
        match kind {
            ParseCacheKind::Full => &mut self.full,
            ParseCacheKind::Head => &mut self.head,
            ParseCacheKind::Tail => &mut self.tail,
        }
    }
}

static PARSE_CACHES: LazyLock<Mutex<ParseCaches>> =
    LazyLock::new(|| Mutex::new(ParseCaches::default()));

/// TS `getCachedOrParse`·`getCachedOrParseHead`·`getCachedOrParseTail`. 수정 시각을 못 읽으면 캐시 없이 파싱한다
pub fn cached_or_parse<T, F>(
    kind: ParseCacheKind,
    host: &SessionHost,
    file_path: &str,
    parse: F,
) -> AiResult<Arc<T>>
where
    T: Any + Send + Sync,
    F: FnOnce() -> AiResult<T>,
{
    let mtime = host.file_mtime_ms(file_path)?;
    cached_or_parse_at(kind, host.ssh_host(), file_path, mtime, now_ms(), parse)
}

/// 수정 시각과 현재 시각을 받아 캐시를 판정한다. 시계를 주입할 수 있도록 [`cached_or_parse`]에서 떼어 냈다
pub fn cached_or_parse_at<T, F>(
    kind: ParseCacheKind,
    ssh_host: Option<&str>,
    file_path: &str,
    mtime: Option<f64>,
    now: f64,
    parse: F,
) -> AiResult<Arc<T>>
where
    T: Any + Send + Sync,
    F: FnOnce() -> AiResult<T>,
{
    let Some(mtime) = mtime else {
        return parse().map(Arc::new);
    };
    let key = cache_key(ssh_host, file_path);
    // 같은 파일을 겹쳐 파싱하면 뒤 호출은 앞 호출이 끝나기를 기다렸다가 그 결과를 함께 쓴다(TS의 Promise 공유)
    let parse_turn = parse_turn(kind, &key);
    let _turn_guard = parse_turn.lock().expect("파싱 차례");
    let previous = {
        let mut caches = PARSE_CACHES.lock().expect("파싱 캐시");
        let cache = caches.select(kind);
        let previous = cache.get(&key).cloned();
        if let Some(hit) = previous
            .as_ref()
            .and_then(|entry| fresh_value::<T>(entry, mtime, now, ssh_host))
        {
            cache.touch(&key);
            return Ok(hit);
        }
        previous
    };

    let value = Arc::new(parse()?);
    let is_mtime_settled = previous.is_some_and(|entry| {
        entry.mtime == mtime && now - entry.parsed_at >= REMOTE_FILE_PARSE_CACHE_TTL_MS
    });
    let entry = CacheEntry {
        mtime,
        parsed_at: now,
        is_mtime_settled,
        value: value.clone(),
    };
    PARSE_CACHES
        .lock()
        .expect("파싱 캐시")
        .select(kind)
        .insert(key, entry);
    Ok(value)
}

type ParseTurns = std::collections::HashMap<(u8, String), Arc<Mutex<()>>>;

static PARSE_TURNS: LazyLock<Mutex<ParseTurns>> = LazyLock::new(|| Mutex::new(ParseTurns::new()));

/// 키별 파싱 차례. 아무도 쓰지 않는 차례는 꺼낼 때 함께 치운다
fn parse_turn(kind: ParseCacheKind, key: &str) -> Arc<Mutex<()>> {
    let mut turns = PARSE_TURNS.lock().expect("파싱 차례 목록");
    turns.retain(|_, turn| Arc::strong_count(turn) > 1);
    turns
        .entry((kind as u8, key.to_string()))
        .or_default()
        .clone()
}

/// 로컬은 수정 시각이 같으면, 원격은 거기에 더해 시각이 확정됐거나 1초가 안 지났으면 캐시를 쓴다
fn fresh_value<T: Any + Send + Sync>(
    entry: &CacheEntry,
    mtime: f64,
    now: f64,
    ssh_host: Option<&str>,
) -> Option<Arc<T>> {
    let is_fresh = entry.mtime == mtime
        && (ssh_host.is_none()
            || entry.is_mtime_settled
            || now - entry.parsed_at < REMOTE_FILE_PARSE_CACHE_TTL_MS);
    if !is_fresh {
        return None;
    }
    entry.value.clone().downcast::<T>().ok()
}

/// TS `JSON.stringify([sshHost || null, filePath])`
fn cache_key(ssh_host: Option<&str>, file_path: &str) -> String {
    let host = ssh_host.map_or_else(|| "null".to_string(), quote_json_string);
    format!("[{host},{}]", quote_json_string(file_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const KINDS: [(ParseCacheKind, &str); 3] = [
        (ParseCacheKind::Full, "full"),
        (ParseCacheKind::Head, "head"),
        (ParseCacheKind::Tail, "tail"),
    ];

    /// 부를 때마다 다음 값을 돌려주고, 불린 횟수를 센다
    struct ScriptedParse {
        values: RefCell<Vec<&'static str>>,
        calls: RefCell<usize>,
    }

    impl ScriptedParse {
        fn new(values: &[&'static str]) -> Self {
            Self {
                values: RefCell::new(values.to_vec()),
                calls: RefCell::new(0),
            }
        }

        fn next(&self) -> AiResult<String> {
            *self.calls.borrow_mut() += 1;
            let mut values = self.values.borrow_mut();
            let value = if values.len() > 1 {
                values.remove(0)
            } else {
                values[0]
            };
            Ok(value.to_string())
        }

        fn set(&self, value: &'static str) {
            *self.values.borrow_mut() = vec![value];
        }

        fn calls(&self) -> usize {
            *self.calls.borrow()
        }
    }

    fn read(
        kind: ParseCacheKind,
        host: Option<&str>,
        file: &str,
        mtime: f64,
        now: f64,
        parse: &ScriptedParse,
    ) -> String {
        cached_or_parse_at(kind, host, file, Some(mtime), now, || parse.next())
            .unwrap()
            .as_ref()
            .clone()
    }

    #[test]
    fn eventually_reparses_remote_last_write_with_unchanged_coarse_mtime() {
        for (kind, name) in KINDS {
            let parse = ScriptedParse::new(&["before", "after"]);
            let file = format!("/remote/{name}-same-second.jsonl");
            let host = Some("fixture-host");
            assert_eq!(read(kind, host, &file, 1_000.0, 10_000.0, &parse), "before");
            assert_eq!(read(kind, host, &file, 1_000.0, 10_500.0, &parse), "before");
            assert_eq!(read(kind, host, &file, 1_000.0, 11_001.0, &parse), "after");
            assert_eq!(parse.calls(), 2);
        }
    }

    #[test]
    fn revalidates_each_remote_coarse_mtime_once() {
        for (kind, name) in KINDS {
            let parse = ScriptedParse::new(&["large-history"]);
            let file = format!("/remote/{name}-settled.jsonl");
            let host = Some("fixture-host");
            read(kind, host, &file, 2_000.0, 20_000.0, &parse);
            read(kind, host, &file, 2_000.0, 21_001.0, &parse);
            read(kind, host, &file, 2_000.0, 30_000.0, &parse);
            assert_eq!(
                read(kind, host, &file, 2_000.0, 600_000.0, &parse),
                "large-history"
            );
            assert_eq!(parse.calls(), 2);

            parse.set("new-record");
            assert_eq!(
                read(kind, host, &file, 3_000.0, 600_000.0, &parse),
                "new-record"
            );
            assert_eq!(parse.calls(), 3);
            read(kind, host, &file, 3_000.0, 601_001.0, &parse);
            read(kind, host, &file, 3_000.0, 900_000.0, &parse);
            assert_eq!(parse.calls(), 4);
        }
    }

    #[test]
    fn keeps_unchanged_local_files_and_invalidates_changed_mtimes() {
        for (kind, name) in KINDS {
            let parse = ScriptedParse::new(&["before", "after"]);
            let file = format!("/local/{name}-precise.jsonl");
            assert_eq!(read(kind, None, &file, 1_000.0, 10_000.0, &parse), "before");
            assert_eq!(read(kind, None, &file, 1_000.0, 50_000.0, &parse), "before");
            assert_eq!(read(kind, None, &file, 1_001.0, 50_000.0, &parse), "after");
            assert_eq!(parse.calls(), 2);
        }
    }

    #[test]
    fn identical_paths_are_isolated_by_execution_host() {
        for (kind, name) in [KINDS[0], KINDS[1]] {
            let file = format!("/same/path/host-{name}.jsonl");
            let read_as = |host: Option<&str>, value: &str| {
                let value = value.to_string();
                cached_or_parse_at(kind, host, &file, Some(1.0), 0.0, || Ok(value)).unwrap()
            };
            let results = [
                read_as(None, "local"),
                read_as(Some("host-a"), "host-a"),
                read_as(Some("host-b"), "host-b"),
            ];
            assert_eq!(
                results.map(|value| value.as_ref().clone()),
                ["local", "host-a", "host-b"]
            );
        }
    }

    #[test]
    fn concurrent_reads_of_an_unchanged_file_share_one_parse() {
        for (kind, name) in [KINDS[0], KINDS[1]] {
            let file = format!("/concurrent/{name}.jsonl");
            let calls = std::sync::atomic::AtomicUsize::new(0);
            let parse = || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(20));
                Ok("parsed".to_string())
            };
            let results = std::thread::scope(|scope| {
                let first = scope.spawn(|| {
                    cached_or_parse_at(kind, None, &file, Some(1.0), 0.0, parse).unwrap()
                });
                let second = scope.spawn(|| {
                    cached_or_parse_at(kind, None, &file, Some(1.0), 0.0, parse).unwrap()
                });
                [first.join().unwrap(), second.join().unwrap()]
            });
            assert_eq!(
                results.map(|value| value.as_ref().clone()),
                ["parsed", "parsed"]
            );
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn isolates_local_and_remote_cache_entries() {
        for (kind, name) in KINDS {
            let file = format!("/same/{name}.jsonl");
            for host in [None, Some("host-a"), Some("host-b")] {
                let expected = host.unwrap_or("local").to_string();
                let value =
                    cached_or_parse_at(
                        kind,
                        host,
                        &file,
                        Some(123.0),
                        0.0,
                        || Ok(expected.clone()),
                    )
                    .unwrap();
                assert_eq!(*value, expected);
            }
            let cached = cached_or_parse_at(kind, Some("host-a"), &file, Some(123.0), 0.0, || {
                Ok("wrong".to_string())
            })
            .unwrap();
            assert_eq!(*cached, "host-a");
        }
    }
}
