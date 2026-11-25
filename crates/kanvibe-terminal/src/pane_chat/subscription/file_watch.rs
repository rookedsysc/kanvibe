//! 대화 기록 파일 감시(TS `fs.watch`). 기록이 바뀌면 다음 주기를 기다리지 않고 pane을 다시 읽게 알린다.

use std::path::Path;
use std::sync::Arc;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// 파일이 바뀌었거나 감시가 실패했을 때 부르는 알림
pub type TranscriptFileSignal = Arc<dyn Fn() + Send + Sync>;

/// 걸어 둔 감시 하나. 값을 버리면 감시를 멈춘다(TS `watcher.close()`)
pub trait TranscriptFileWatch: Send {}

pub trait TranscriptFileWatchers: Send + Sync {
    /// 파일 하나를 감시한다. 감시를 걸지 못하면 `Err`이고, 건 뒤에 감시가 죽으면 `on_error`를 부른다
    fn watch(
        &self,
        file_path: &str,
        on_change: TranscriptFileSignal,
        on_error: TranscriptFileSignal,
    ) -> Result<Box<dyn TranscriptFileWatch>, String>;
}

/// OS 파일 이벤트로 감시하는 실제 구현
#[derive(Debug, Clone, Copy, Default)]
pub struct NotifyTranscriptFileWatchers;

struct NotifyTranscriptFileWatch {
    _watcher: RecommendedWatcher,
}

impl TranscriptFileWatch for NotifyTranscriptFileWatch {}

impl TranscriptFileWatchers for NotifyTranscriptFileWatchers {
    fn watch(
        &self,
        file_path: &str,
        on_change: TranscriptFileSignal,
        on_error: TranscriptFileSignal,
    ) -> Result<Box<dyn TranscriptFileWatch>, String> {
        // 읽기만 한 접근 이벤트는 fs.watch가 알리지 않는다. 우리가 기록을 읽을 때마다 다시 읽기를 부르지 않도록 거른다
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
                Ok(_) => on_change(),
                Err(_) => on_error(),
            })
            .map_err(|error| error.to_string())?;
        watcher
            .watch(Path::new(file_path), RecursiveMode::NonRecursive)
            .map_err(|error| error.to_string())?;
        Ok(Box::new(NotifyTranscriptFileWatch { _watcher: watcher }))
    }
}
