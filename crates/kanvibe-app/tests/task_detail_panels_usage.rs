//! 작업 상세 AI 사용량 패널(TS `AiUsagePanel`)을 실제 앱으로 열어, 사용량 조회 응답에 따라 보이는 글자와 동작을 확인한다.
//! 조회 결과는 QA 고장 주입의 값 대체로 정한다(TS 테스트의 `useAiUsage` 모의와 같은 자리).

mod task_detail_panels_support;

use std::time::Duration;

use gpui_kit::TestAppContext;
use gpui_kit::gpui;
use kanvibe_app::qa::faults::FaultMode;
use serde_json::{Value, json};
use task_detail_panels_support::*;

const SNAPSHOT: &str = "aiUsage.getAiUsageSnapshot";
const CACHED: &str = "aiUsage.getCachedAiUsageSnapshot";

fn claude_account(overrides: Value) -> Value {
    let mut account = json!({
        "provider": "claude",
        "accountId": "personal",
        "label": "me@example.com",
        "status": "ok",
        "planName": null,
        "windows": [
            { "kind": "session", "modelName": null, "usedPercent": 22, "resetsAt": null },
            { "kind": "weekly", "modelName": null, "usedPercent": 95, "resetsAt": null }
        ],
        "reason": null,
        "fetchedAt": "2026-01-15T02:00:00.000Z"
    });
    if let (Some(account), Some(overrides)) = (account.as_object_mut(), overrides.as_object()) {
        for (key, value) in overrides {
            account.insert(key.clone(), value.clone());
        }
    }
    account
}

fn default_accounts() -> Value {
    json!([
        claude_account(json!({})),
        {
            "provider": "codex", "accountId": "codex-seat", "label": "codex@example.com", "status": "ok",
            "planName": "pro",
            "windows": [{ "kind": "weekly", "modelName": null, "usedPercent": 35, "resetsAt": null }],
            "reason": null, "fetchedAt": "2026-01-15T02:00:00.000Z"
        },
        {
            "provider": "gemini", "accountId": "gemini", "label": "gemini", "status": "unavailable",
            "planName": null, "windows": [], "reason": "missing-credentials",
            "fetchedAt": "2026-01-15T02:00:00.000Z"
        }
    ])
}

fn snapshot(accounts: Value) -> Value {
    json!({ "fetchedAt": "2026-01-15T02:00:00.000Z", "accounts": accounts })
}

/// 작업 하나를 열고 사용량 패널(`Mod+0`)을 연다
fn open_usage(cx: &mut TestAppContext, setup: impl FnOnce(&mut Detail)) -> Detail {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let task = task(&database, "사용량 확인", |_| {});
    let mut detail = open_detail(cx, &task.id);
    detail.substitute(CACHED, Value::Null);
    setup(&mut detail);
    detail.keys("ctrl-0");
    detail
}

fn open_usage_with(cx: &mut TestAppContext, accounts: Value) -> Detail {
    open_usage(cx, |detail| detail.substitute(SNAPSHOT, snapshot(accounts)))
}

#[gpui::test]
fn each_provider_shows_used_percent_and_plan(cx: &mut TestAppContext) {
    let mut detail = open_usage_with(cx, default_accounts());

    assert!(detail.has_text("22%"));
    assert!(detail.has_text("95%"));
    assert!(detail.has_text("35%"));
    assert!(detail.has_text("PRO"));
}

#[gpui::test]
fn provider_without_credentials_shows_the_reason_instead_of_bars(cx: &mut TestAppContext) {
    let mut detail = open_usage_with(cx, default_accounts());

    assert!(detail.has_text("Gemini"));
    assert!(detail.has_text("로그인되어 있지 않습니다."));
}

#[gpui::test]
fn carried_card_shows_bars_and_the_failure_reason_together(cx: &mut TestAppContext) {
    let accounts = json!([claude_account(
        json!({ "status": "error", "reason": "rate-limited" })
    )]);
    let mut detail = open_usage_with(cx, accounts);

    assert!(detail.has_text("22%"));
    assert!(detail.has_text("요청이 너무 잦아 잠시 뒤 다시 시도해야 합니다."));
}

#[gpui::test]
fn carried_values_say_when_they_were_read(cx: &mut TestAppContext) {
    let accounts = json!([claude_account(json!({
        "status": "error", "reason": "rate-limited", "fetchedAt": "2026-01-14T01:00:00.000Z"
    }))]);
    let mut detail = open_usage_with(cx, accounts);

    // 시각은 시스템 locale·시간대로 적는다(en-US·UTC면 `1/14, 1:00 AM`). 표기 자체는 `locale_format`이 확인한다
    let fetched_at =
        kanvibe_app::views::settings::local_time::LocalDateTime::from_epoch_ms(1_768_352_400_000.0)
            .unwrap();
    let read_at = kanvibe_app::locale_format::format_month_day_time_hm(fetched_at).unwrap();
    assert!(detail.has_text(&format!("{read_at} 기준 값입니다.")));
}

#[gpui::test]
fn successful_cards_do_not_say_they_were_carried(cx: &mut TestAppContext) {
    let mut detail = open_usage_with(cx, default_accounts());

    assert!(!detail.has_text_containing("기준 값입니다."));
}

#[gpui::test]
fn a_single_account_still_names_itself(cx: &mut TestAppContext) {
    let mut detail = open_usage_with(cx, default_accounts());

    assert!(detail.has_text("me@example.com"));
}

#[gpui::test]
fn several_accounts_of_one_provider_are_named(cx: &mut TestAppContext) {
    let accounts = json!([
        claude_account(json!({})),
        claude_account(json!({
            "accountId": "work", "label": "work@example.com",
            "windows": [{ "kind": "session", "modelName": null, "usedPercent": 7, "resetsAt": null }]
        }))
    ]);
    let mut detail = open_usage_with(cx, accounts);

    assert!(detail.has_text("me@example.com"));
    assert!(detail.has_text("work@example.com"));
    assert!(detail.has_text("7%"));
}

#[gpui::test]
fn refreshing_over_saved_values_says_refreshing_not_loading(cx: &mut TestAppContext) {
    let mut detail = open_usage(cx, |detail| {
        detail.clear_fault(CACHED);
        detail.substitute(CACHED, snapshot(default_accounts()));
        detail.hang(SNAPSHOT);
    });

    assert!(detail.has_text("사용량을 갱신하는 중"));
    assert!(!detail.has_text("사용량을 불러오는 중"));
    assert!(detail.has_text("22%"));
}

#[gpui::test]
fn loading_shows_only_while_nothing_can_be_shown(cx: &mut TestAppContext) {
    let mut detail = open_usage(cx, |detail| detail.hang(SNAPSHOT));

    assert!(detail.has_text("사용량을 불러오는 중"));
}

#[gpui::test]
fn refresh_button_reads_again(cx: &mut TestAppContext) {
    let mut detail = open_usage_with(cx, default_accounts());
    assert!(detail.has_text("22%"));

    detail.clear_fault(SNAPSHOT);
    detail.substitute(
        SNAPSHOT,
        snapshot(json!([claude_account(json!({
            "windows": [{ "kind": "session", "modelName": null, "usedPercent": 64, "resetsAt": null }]
        }))])),
    );
    detail.click_role("button", "새로고침");

    assert!(detail.has_text("64%"));
}

#[gpui::test]
fn refresh_is_blocked_while_reading(cx: &mut TestAppContext) {
    let mut detail = open_usage(cx, |detail| detail.hang(SNAPSHOT));
    detail.clear_fault(SNAPSHOT);
    detail.substitute(SNAPSHOT, snapshot(default_accounts()));

    detail.click_role("button", "새로고침");

    // 막힌 버튼은 새 조회를 내지 않아 첫 조회가 끝나기 전까지 값이 보이지 않는다
    assert!(detail.has_text("사용량을 불러오는 중"));
    assert!(!detail.has_text("22%"));
}

#[gpui::test]
fn total_failure_without_values_is_reported(cx: &mut TestAppContext) {
    let mut detail = open_usage(cx, |detail| detail.fail(SNAPSHOT, "ipc down"));

    assert!(detail.has_text("사용량을 불러오지 못했습니다."));
}

#[gpui::test]
fn sign_in_reasons_link_to_the_accounts_screen(cx: &mut TestAppContext) {
    let accounts = json!([claude_account(json!({
        "status": "unavailable", "windows": [], "reason": "expired-credentials"
    }))]);
    let mut detail = open_usage_with(cx, accounts);

    assert!(detail.has_role_name("link", "계정 관리"));
}

#[gpui::test]
fn unrelated_reasons_do_not_link_to_the_accounts_screen(cx: &mut TestAppContext) {
    let accounts = json!([claude_account(json!({
        "status": "error", "windows": [], "reason": "rate-limited"
    }))]);
    let mut detail = open_usage_with(cx, accounts);

    assert!(!detail.has_role_name("link", "계정 관리"));
}

#[gpui::test]
fn closed_panel_reads_nothing(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let task = task(&database, "사용량 확인", |_| {});
    let mut detail = open_detail(cx, &task.id);
    detail.fail(SNAPSHOT, "should not be called");
    detail.fail(CACHED, "should not be called");
    detail.draw();

    assert!(!detail.has_text("사용량을 불러오지 못했습니다."));
    assert!(!detail.has_role_name("button", "새로고침"));
}

#[gpui::test]
fn closed_panel_stops_reading(cx: &mut TestAppContext) {
    let mut detail = open_usage_with(cx, default_accounts());
    detail.keys("ctrl-0");
    detail.clear_fault(SNAPSHOT);
    // 닫힌 뒤 자동 조회가 나갔다면 다시 열 때의 조회는 세 번째가 되어 실패를 받는다
    detail.fault_after(
        SNAPSHOT,
        FaultMode::Fail,
        Some("read while closed"),
        None,
        Some(2),
    );
    let refreshed = snapshot(json!([claude_account(json!({
        "windows": [{ "kind": "session", "modelName": null, "usedPercent": 64, "resetsAt": null }]
    }))]));
    detail.fault_after(SNAPSHOT, FaultMode::Result, None, Some(refreshed), Some(1));

    detail.advance_clock(Duration::from_secs(11 * 60));
    detail.keys("ctrl-0");

    assert!(detail.has_text("64%"));
    assert!(!detail.has_text("사용량을 불러오지 못했습니다."));
}
