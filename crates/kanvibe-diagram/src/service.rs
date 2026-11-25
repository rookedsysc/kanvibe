//! 모바일 다이어그램 렌더 서비스(`mobileDiagramRenderService.ts`).
//!
//! 같은 요청은 한 번만 그리고(진행 중이면 기다렸다가 같은 결과를 받는다), 그리는 작업은 한 번에 하나씩 돈다.
//! 진행 중인 서로 다른 요청이 4개면 다섯 번째는 429로 거절한다. 성공한 이미지만 최대 16개·16 MiB까지
//! 가장 오래 안 쓴 것부터 버리는 캐시에 둔다. Electron 창이 없으므로 503(렌더러 없음·15초 초과)은 생기지 않는다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use indexmap::IndexMap;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::contract::{
    self, BUSY_MESSAGE, IMAGE_FAILED_MESSAGE, MobileDiagramError, MobileDiagramImage,
    MobileDiagramInput, SYNTAX_MESSAGE,
};
use crate::raster;
use crate::text::FontBook;
use crate::theme::Palette;

/// kanvibe-server가 모바일 라우트에서 부르는 렌더러
pub trait DiagramRenderer: Send + Sync {
    /// 요청 본문(JSON)을 검증하고 이미지를 돌려준다. 오류의 `status_code`·`message`를 그대로 응답에 쓴다
    fn render_mobile_diagram(
        &self,
        request: &Value,
    ) -> Result<MobileDiagramImage, MobileDiagramError>;
}

type RenderResult = Result<MobileDiagramImage, MobileDiagramError>;

#[derive(Default)]
struct InFlight {
    result: Mutex<Option<RenderResult>>,
    done: Condvar,
}

#[derive(Default)]
struct CacheState {
    images: IndexMap<String, (MobileDiagramImage, usize)>,
    bytes: usize,
    in_flight: HashMap<String, Arc<InFlight>>,
}

/// 실제로 이미지를 만드는 쪽. 시험에서는 가짜로 바꾼다
pub trait DiagramRasterizer: Send + Sync {
    /// 인코딩한 이미지 바이트와 줄이기 전 폭
    fn rasterize(
        &self,
        input: &MobileDiagramInput,
    ) -> Result<(Vec<u8>, Option<u32>), MobileDiagramError>;
}

/// 네이티브 Mermaid 장면을 tiny-skia로 그려 JPEG로 만든다
pub struct NativeRasterizer {
    fonts: FontBook,
}

impl NativeRasterizer {
    pub fn new(fonts: FontBook) -> Self {
        Self { fonts }
    }
}

impl DiagramRasterizer for NativeRasterizer {
    fn rasterize(
        &self,
        input: &MobileDiagramInput,
    ) -> Result<(Vec<u8>, Option<u32>), MobileDiagramError> {
        let scene = crate::render_scene(&input.definition, input.theme, &self.fonts)
            .map_err(|_| MobileDiagramError::new(422, SYNTAX_MESSAGE))?;
        let palette = Palette::for_theme(input.theme);
        let (pixmap, layout) =
            raster::rasterize_mobile(&scene, &self.fonts, palette.page_background, input.width)
                .ok_or_else(|| MobileDiagramError::new(500, IMAGE_FAILED_MESSAGE))?;
        let encoded = raster::encode_jpeg(&pixmap)
            .map_err(|_| MobileDiagramError::new(500, IMAGE_FAILED_MESSAGE))?;
        Ok((
            encoded,
            Some(layout.natural_width).filter(|width| *width > 0),
        ))
    }
}

pub struct MobileDiagramService<R: DiagramRasterizer> {
    rasterizer: R,
    state: Mutex<CacheState>,
    worker: Mutex<()>,
}

pub type NativeDiagramRenderer = MobileDiagramService<NativeRasterizer>;

impl NativeDiagramRenderer {
    pub fn with_fonts(fonts: FontBook) -> Self {
        MobileDiagramService::new(NativeRasterizer::new(fonts))
    }
}

fn cache_key(input: &MobileDiagramInput) -> String {
    let digest = Sha256::digest(input.canonical_json().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

enum Admission {
    Cached(MobileDiagramImage),
    Waiting(Arc<InFlight>),
    Owner(Arc<InFlight>),
}

impl<R: DiagramRasterizer> MobileDiagramService<R> {
    pub fn new(rasterizer: R) -> Self {
        Self {
            rasterizer,
            state: Mutex::new(CacheState::default()),
            worker: Mutex::new(()),
        }
    }

    fn admit(&self, key: &str) -> Result<Admission, MobileDiagramError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = state.images.shift_remove(key) {
            let image = entry.0.clone();
            state.images.insert(key.to_string(), entry);
            return Ok(Admission::Cached(image));
        }
        if let Some(existing) = state.in_flight.get(key) {
            return Ok(Admission::Waiting(existing.clone()));
        }
        if state.in_flight.len() >= contract::PENDING_LIMIT {
            return Err(MobileDiagramError::new(429, BUSY_MESSAGE));
        }
        let job = Arc::new(InFlight::default());
        state.in_flight.insert(key.to_string(), job.clone());
        Ok(Admission::Owner(job))
    }

    fn run(&self, input: &MobileDiagramInput) -> RenderResult {
        let _turn = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (encoded, natural_width) = self.rasterizer.rasterize(input)?;
        let mut image = contract::validate_mobile_diagram_image(&encoded, input.width)?;
        image.natural_width = natural_width;
        Ok(image)
    }

    fn finish(&self, key: &str, job: &InFlight, result: &RenderResult) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.in_flight.remove(key);
        if let Ok(image) = result {
            let bytes = image.image_base64.len();
            state.images.insert(key.to_string(), (image.clone(), bytes));
            state.bytes += bytes;
            while state.images.len() > contract::CACHE_ENTRIES_LIMIT
                || state.bytes > contract::CACHE_BYTES_LIMIT
            {
                let Some((_, (_, evicted))) = state.images.shift_remove_index(0) else {
                    break;
                };
                state.bytes -= evicted;
            }
        }
        drop(state);
        *job.result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result.clone());
        job.done.notify_all();
    }

    fn wait(job: &InFlight) -> RenderResult {
        let mut result = job
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while result.is_none() {
            result = job
                .done
                .wait(result)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        result.clone().expect("checked")
    }
}

impl<R: DiagramRasterizer> DiagramRenderer for MobileDiagramService<R> {
    fn render_mobile_diagram(&self, request: &Value) -> RenderResult {
        let input = contract::parse_mobile_diagram_input(request)?;
        let key = cache_key(&input);
        match self.admit(&key)? {
            Admission::Cached(image) => Ok(image),
            Admission::Waiting(job) => Self::wait(&job),
            Admission::Owner(job) => {
                let result = self.run(&input);
                self.finish(&key, &job, &result);
                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    fn png(width: u32, height: u32, extra: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; 33 + extra];
        bytes[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    }

    fn request(definition: &str) -> Value {
        json!({ "definition": definition, "theme": "light", "width": 640 })
    }

    /// 정해 둔 결과를 돌려주며 호출 수를 센다. `gate`가 있으면 결과를 받을 때까지 기다린다
    type RasterResult = Result<(Vec<u8>, Option<u32>), MobileDiagramError>;

    #[derive(Default)]
    struct FakeRasterizer {
        calls: AtomicUsize,
        results: Mutex<Vec<RasterResult>>,
        gate: Mutex<Option<mpsc::Receiver<()>>>,
        image_extra: usize,
    }

    impl DiagramRasterizer for FakeRasterizer {
        fn rasterize(
            &self,
            _input: &MobileDiagramInput,
        ) -> Result<(Vec<u8>, Option<u32>), MobileDiagramError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = self.gate.lock().expect("gate").as_ref() {
                gate.recv().expect("release");
            }
            let mut results = self.results.lock().expect("results");
            if results.is_empty() {
                Ok((png(640, 120, self.image_extra), None))
            } else {
                results.remove(0)
            }
        }
    }

    fn pending(service: &MobileDiagramService<FakeRasterizer>) -> usize {
        service.state.lock().expect("state").in_flight.len()
    }

    fn wait_until(condition: impl Fn() -> bool) {
        for _ in 0..500 {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("조건이 끝내 참이 되지 않았습니다");
    }

    // parity: TS-main-mobileDiagramRenderService-c54882
    #[test]
    fn evicts_by_encoded_image_bytes_before_the_entry_count_is_reached() {
        let fake = FakeRasterizer {
            image_extra: 3 * 1024 * 1024 - 33,
            ..FakeRasterizer::default()
        };
        let service = MobileDiagramService::new(fake);
        let base = request("flowchart LR\nA[Start]-->B[Done]");
        service.render_mobile_diagram(&base).expect("first");
        for index in 0..4 {
            service
                .render_mobile_diagram(&request(&format!("flowchart LR\nMemory{index}-->Bound")))
                .expect("fill");
        }
        service.render_mobile_diagram(&base).expect("again");
        assert_eq!(service.rasterizer.calls.load(Ordering::SeqCst), 6);
    }

    // parity: TS-main-mobileDiagramRenderService-429ad3
    #[test]
    fn deduplicates_concurrent_requests_and_serves_cached_copies() {
        let (release, gate) = mpsc::channel();
        let fake = FakeRasterizer {
            gate: Mutex::new(Some(gate)),
            ..FakeRasterizer::default()
        };
        let service = Arc::new(MobileDiagramService::new(fake));
        let base = request("flowchart LR\nA[Start]-->B[Done]");
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (service, base) = (service.clone(), base.clone());
                thread::spawn(move || service.render_mobile_diagram(&base))
            })
            .collect();
        wait_until(|| service.rasterizer.calls.load(Ordering::SeqCst) == 1);
        release.send(()).expect("release");
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("join").expect("image"))
            .collect();
        assert_eq!(results[0], results[1]);
        assert_eq!(
            service.render_mobile_diagram(&base).expect("cached").height,
            120
        );
        assert_eq!(service.rasterizer.calls.load(Ordering::SeqCst), 1);
    }

    // parity: TS-main-mobileDiagramRenderService-9329d2
    #[test]
    fn runs_one_worker_admits_three_waiting_jobs_and_rejects_a_fifth() {
        let (release, gate) = mpsc::channel();
        let fake = FakeRasterizer {
            gate: Mutex::new(Some(gate)),
            ..FakeRasterizer::default()
        };
        let service = Arc::new(MobileDiagramService::new(fake));
        let handles: Vec<_> = (0..4)
            .map(|index| {
                let service = service.clone();
                thread::spawn(move || {
                    service.render_mobile_diagram(&request(&format!("flowchart LR\nA-->B{index}")))
                })
            })
            .collect();
        wait_until(|| pending(&service) == 4);
        let rejected = service
            .render_mobile_diagram(&request("flowchart LR\nX-->Y"))
            .expect_err("busy");
        assert_eq!(
            (rejected.status_code, rejected.message.as_str()),
            (429, contract::BUSY_MESSAGE)
        );
        for expected_calls in 1..=4 {
            wait_until(|| service.rasterizer.calls.load(Ordering::SeqCst) == expected_calls);
            release.send(()).expect("release");
        }
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().expect("join"))
                .filter(Result::is_ok)
                .count(),
            4
        );
    }

    // parity: TS-main-mobileDiagramRenderService-c0e6c7
    #[test]
    fn carries_the_natural_width_and_omits_it_when_unknown() {
        let fake = FakeRasterizer::default();
        fake.results
            .lock()
            .expect("results")
            .push(Ok((png(640, 120, 0), Some(1100))));
        let service = MobileDiagramService::new(fake);
        let measured = service
            .render_mobile_diagram(&request("flowchart LR\nA[Start]-->B[Done]"))
            .expect("measured");
        assert_eq!((measured.width, measured.natural_width), (640, Some(1100)));
        let unknown = service
            .render_mobile_diagram(&request("flowchart LR\nOld-->Desktop"))
            .expect("unknown");
        assert_eq!(unknown.natural_width, None);
        assert!(unknown.to_response_json().get("naturalWidth").is_none());
    }

    // parity: TS-main-mobileDiagramRenderService-d14451
    #[test]
    fn does_not_cache_failures_and_lets_the_next_request_run() {
        let fake = FakeRasterizer::default();
        fake.results
            .lock()
            .expect("results")
            .push(Err(MobileDiagramError::new(503, "unavailable")));
        let service = MobileDiagramService::new(fake);
        let base = request("flowchart LR\nA[Start]-->B[Done]");
        assert_eq!(
            service
                .render_mobile_diagram(&base)
                .expect_err("failure")
                .status_code,
            503
        );
        assert_eq!(
            service.render_mobile_diagram(&base).expect("second").width,
            640
        );
        assert_eq!(service.rasterizer.calls.load(Ordering::SeqCst), 2);
    }

    // parity: TS-main-mobileDiagramRenderService-bc4bee
    #[test]
    fn keeps_theme_and_width_distinct_and_evicts_the_oldest_of_seventeen() {
        let service = MobileDiagramService::new(FakeRasterizer::default());
        let base = request("flowchart LR\nA[Start]-->B[Done]");
        service.render_mobile_diagram(&base).expect("light");
        let mut dark = base.clone();
        dark["theme"] = json!("dark");
        service.render_mobile_diagram(&dark).expect("dark");
        for index in 0..15 {
            service
                .render_mobile_diagram(&request(&format!("flowchart LR\nN{index}-->Z")))
                .expect("fill");
        }
        service.render_mobile_diagram(&base).expect("again");
        assert_eq!(service.rasterizer.calls.load(Ordering::SeqCst), 18);
    }

    fn native() -> NativeDiagramRenderer {
        NativeDiagramRenderer::with_fonts(crate::text::tests::bundled_fonts())
    }

    // parity: TS-main-mobileDiagramWorker-33a075
    #[test]
    fn reports_the_natural_width_of_the_rendered_diagram() {
        let image = native()
            .render_mobile_diagram(
                &json!({ "definition": "graph TD; A-->B", "theme": "light", "width": 400 }),
            )
            .expect("image");
        assert_eq!(
            (image.width, image.mime_type, image.natural_width),
            (400, "image/jpeg", Some(87))
        );
    }

    // parity: TS-main-mobileDiagramWorker-48037a
    #[test]
    fn maps_syntax_failures_to_422() {
        let error = native()
            .render_mobile_diagram(&request("flowchart LR\nA -->"))
            .expect_err("syntax");
        assert_eq!(
            (error.status_code, error.message.as_str()),
            (422, contract::SYNTAX_MESSAGE)
        );
    }
}
