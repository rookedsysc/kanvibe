use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use serde_json::json;

use super::*;

/// 요청 횟수를 세고 정해 둔 응답을 돌려준다. `gate`가 있으면 테스트가 풀어 줄 때까지 응답을 미룬다
struct FakeReleaseFeedClient {
    response: Result<Value, String>,
    call_count: AtomicUsize,
    started: Mutex<Option<mpsc::Sender<()>>>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
}

impl FakeReleaseFeedClient {
    fn new(response: Result<Value, String>) -> Arc<Self> {
        Arc::new(Self {
            response,
            call_count: AtomicUsize::new(0),
            started: Mutex::new(None),
            gate: Mutex::new(None),
        })
    }

    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl ReleaseFeedClient for FakeReleaseFeedClient {
    fn fetch_releases(&self) -> Result<Value, String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        if let Some(started) = self.started.lock().unwrap().take() {
            started.send(()).unwrap();
        }
        if let Some(gate) = self.gate.lock().unwrap().take() {
            gate.recv().unwrap();
        }
        self.response.clone()
    }
}

fn release(tag_name: &str, name: &str, body: &str) -> Value {
    json!({
        "tag_name": tag_name,
        "name": name,
        "body": body,
        "html_url": format!("https://github.com/rookedsysc/kanvibe/releases/tag/{tag_name}"),
    })
}

fn service_with(client: Arc<FakeReleaseFeedClient>) -> ReleaseUpdateService {
    ReleaseUpdateService::new(client, "1.0.0", false)
}

// parity: TS-main-releaseUpdateService-ae0bed
#[test]
fn development_runs_report_zero_as_the_current_version() {
    let service =
        ReleaseUpdateService::new(FakeReleaseFeedClient::new(Ok(json!([]))), "1.2.3", true);

    assert_eq!(service.current_release_version(), "0.0.0");
}

// parity: TS-main-releaseUpdateService-62217e
#[test]
fn packaged_runs_report_the_app_version() {
    let service =
        ReleaseUpdateService::new(FakeReleaseFeedClient::new(Ok(json!([]))), "1.2.3", false);

    assert_eq!(service.current_release_version(), "1.2.3");
}

// parity: TS-main-releaseUpdateService-509244
#[test]
fn selects_the_highest_non_draft_release_above_the_current_version() {
    let mut draft = release("1.2.0", "Draft release", "Hidden");
    draft["draft"] = json!(true);
    let mut feature = release("1.1.0", "Feature release", "Feature notes");
    feature["published_at"] = json!("2026-05-10T00:00:00Z");

    let selected = select_latest_release_update(
        &[release("1.0.1", "Small fix", "Patch notes"), draft, feature],
        "1.0.0",
    );

    assert_eq!(
        selected,
        Some(ReleaseUpdate {
            version: "1.1.0".into(),
            tag_name: "1.1.0".into(),
            name: "Feature release".into(),
            body: "Feature notes".into(),
            html_url: "https://github.com/rookedsysc/kanvibe/releases/tag/1.1.0".into(),
            published_at: Some("2026-05-10T00:00:00Z".into()),
        })
    );
}

// parity: TS-main-releaseUpdateService-df8fc4
#[test]
fn reports_no_update_when_the_github_request_fails() {
    let client =
        FakeReleaseFeedClient::new(Err("GitHub releases request failed with status 403".into()));
    let service = service_with(client);

    let result = service.check_for_release_update();

    assert_eq!(
        result,
        ReleaseUpdateCheckResult {
            current_version: "1.0.0".into(),
            is_update_available: false,
            release: None,
            error: Some("GitHub releases request failed with status 403".into()),
        }
    );
}

// parity: TS-main-releaseUpdateService-8754ed
#[test]
fn returns_release_details_from_github_releases() {
    let client = FakeReleaseFeedClient::new(Ok(json!([
        release("1.0.0", "Current", ""),
        release("1.0.2", "New release", "Release notes"),
    ])));
    let service = service_with(client);

    let result = service.check_for_release_update();

    let found = result.release.expect("새 릴리스");
    assert!(result.is_update_available);
    assert_eq!(result.current_version, "1.0.0");
    assert_eq!(
        (
            found.version.as_str(),
            found.tag_name.as_str(),
            found.name.as_str(),
            found.body.as_str()
        ),
        ("1.0.2", "1.0.2", "New release", "Release notes")
    );
}

// parity: TS-main-releaseUpdateService-aea597
#[test]
fn concurrent_checks_share_one_request_without_consuming_the_release() {
    let client = FakeReleaseFeedClient::new(Ok(json!([release(
        "1.0.2",
        "New release",
        "Release notes"
    )])));
    let (started_sender, started_receiver) = mpsc::channel();
    let (gate_sender, gate_receiver) = mpsc::channel();
    *client.started.lock().unwrap() = Some(started_sender);
    *client.gate.lock().unwrap() = Some(gate_receiver);
    let service = service_with(Arc::clone(&client));

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| service.check_for_release_update());
        started_receiver.recv().unwrap();
        let second = scope.spawn(|| service.check_for_release_update());
        std::thread::sleep(Duration::from_millis(30));
        gate_sender.send(()).unwrap();
        (first.join().unwrap(), second.join().unwrap())
    });

    assert_eq!(client.call_count(), 1);
    for result in [first, second] {
        assert!(result.is_update_available);
        assert_eq!(result.current_version, "1.0.0");
        assert_eq!(result.release.unwrap().version, "1.0.2");
    }
}

// parity: TS-main-releaseUpdateService-48423c
#[test]
fn hides_a_cached_release_after_a_window_claims_it() {
    let client = FakeReleaseFeedClient::new(Ok(json!([release(
        "1.0.2",
        "New release",
        "Release notes"
    )])));
    let service = service_with(Arc::clone(&client));

    let before_claim = service.check_for_release_update();
    let is_first_claim = service.claim_release_update_version("1.0.2");
    let is_second_claim = service.claim_release_update_version("1.0.2");
    let after_claim = service.check_for_release_update();

    assert_eq!(before_claim.release.unwrap().version, "1.0.2");
    assert!(is_first_claim);
    assert!(!is_second_claim);
    assert!(!after_claim.is_update_available);
    assert_eq!(after_claim.release, None);
    assert_eq!(client.call_count(), 1);
}

#[test]
fn checks_again_once_the_cached_result_is_an_hour_old() {
    let client = FakeReleaseFeedClient::new(Ok(json!([])));
    let base = Instant::now();
    let elapsed = Arc::new(Mutex::new(Duration::ZERO));
    let clock_elapsed = Arc::clone(&elapsed);
    let service = ReleaseUpdateService::with_clock(
        Arc::clone(&client) as Arc<dyn ReleaseFeedClient>,
        "1.0.0",
        false,
        move || base + *clock_elapsed.lock().unwrap(),
    );

    service.check_for_release_update();
    *elapsed.lock().unwrap() = RELEASE_UPDATE_CHECK_CACHE - Duration::from_millis(1);
    service.check_for_release_update();
    *elapsed.lock().unwrap() = RELEASE_UPDATE_CHECK_CACHE * 2;
    service.check_for_release_update();

    assert_eq!(client.call_count(), 2);
}
