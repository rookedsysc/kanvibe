//! `/:locale/ai-accounts` AI 계정 화면(TS `AiAccountsRoute`). provider(Claude·Codex·Gemini)별 카드에 계정 행·로그인 상태·
//! 사용량 막대를 보이고, 카드 안에서 계정을 추가하면 이어서 내장 터미널로 로그인 CLI를 띄운다.
//! 목록을 처음 읽지 못하면 "계정 없음" 대신 사유와 재시도를 보인다.

pub mod backend;
pub mod login_terminal;
pub mod model;
pub mod usage_block;

use std::sync::Arc;

use gpui_kit::base::input::InputState;
use gpui_kit::{
    AnyElement, AppContext, Context, Div, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, ScrollHandle, Styled, Subscription, Task, Window, div, px,
};
use kanvibe_ai::accounts::account_service::AiAccountSummary;
use kanvibe_ai::javascript::parse_date_ms;
use kanvibe_ai::usage::types::AiUsageProvider;
use kanvibe_domain::i18n::Translator;

use crate::components::TailwindText;
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::ScreenContext;
use crate::views::settings::bulk_session_type::window_confirm_prompt;
use crate::views::settings::controls::set_window_title_once;
use crate::views::settings::controls::{
    centered_message, inline_load_error, make_pressable, overflow_scroll_area, pressable,
    scrollbar_color, text_link,
};
use crate::views::settings::service_call::call_service;
use crate::views::settings::text_field::create_text_field;
use backend::{AiAccountsBackend, ai_accounts_backend};
use login_terminal::{AiAccountLoginTerminal, LoginExited};
use model::{AiAccountsModel, provider_index, provider_title};

/// `max-w-2xl`
const CONTENT_MAX_WIDTH: f32 = 672.0;
/// TS `AI_USAGE_REFRESH_INTERVAL_MS`: 사용량 자동 조회 주기
const USAGE_REFRESH_INTERVAL_MS: i64 = 5 * 60 * 1000;

pub struct AiAccountsView {
    screen: ScreenContext,
    /// 창 제목을 걸었는지(TS `document.title`)
    is_title_set: bool,
    pub model: AiAccountsModel,
    backend: Arc<dyn AiAccountsBackend>,
    /// provider 순서의 계정 이름 입력란. 창이 있어야 만들 수 있어 첫 render에서 만든다
    name_inputs: Option<[Entity<InputState>; 3]>,
    login_terminal: Option<Entity<AiAccountLoginTerminal>>,
    scroll_handle: ScrollHandle,
    _input_subscriptions: Vec<Subscription>,
    _login_subscription: Option<Subscription>,
    _usage_timer: Option<Task<()>>,
    _requests: Vec<Task<()>>,
}

impl AiAccountsView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let backend = ai_accounts_backend(cx);
        let mut view = Self {
            screen,
            is_title_set: false,
            model: AiAccountsModel::default(),
            backend,
            name_inputs: None,
            login_terminal: None,
            scroll_handle: ScrollHandle::new(),
            _input_subscriptions: Vec::new(),
            _login_subscription: None,
            _usage_timer: None,
            _requests: Vec::new(),
        };
        view.load_accounts(cx);
        view.show_cached_usage(cx);
        view.refresh_usage(true, cx);
        view
    }

    /// 열려 있는 로그인 터미널
    pub fn login_terminal(&self) -> Option<Entity<AiAccountLoginTerminal>> {
        self.login_terminal.clone()
    }

    fn translator(&self, namespace: &'static str) -> Translator {
        Translator::new(self.screen.route.locale, namespace)
    }

    fn track(&mut self, task: Task<()>) {
        self._requests.retain(|request| !request.is_ready());
        self._requests.push(task);
    }

    pub fn load_accounts(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let request = call_service(cx, "aiAccounts.listAiAccounts", "[]".into(), move || {
            backend.list_ai_accounts()
        });
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.model
                    .finish_list(result.map_err(|failure| failure.reason));
                cx.notify();
            });
        }));
    }

    fn show_cached_usage(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let request = call_service(
            cx,
            "aiUsage.getCachedAiUsageSnapshot",
            "[]".into(),
            move || backend.get_cached_ai_usage_snapshot(),
        );
        self.track(cx.spawn(async move |this, cx| {
            if let Ok(cached) = request.await {
                let _ = this.update(cx, |this, cx| {
                    this.model.usage.show_cached(cached);
                    cx.notify();
                });
            }
        }));
    }

    /// 사용량을 다시 읽는다. 새로고침 버튼과 로그인 직후는 나눠 쓰는 결과로 답하지 않는다
    pub fn refresh_usage(&mut self, can_reuse_recent_snapshot: bool, cx: &mut Context<Self>) {
        if self.model.usage.is_refreshing {
            return;
        }
        self.model.usage.is_refreshing = true;
        let backend = self.backend.clone();
        let request = call_service(
            cx,
            "aiUsage.getAiUsageSnapshot",
            format!("[{{\"canReuseRecentSnapshot\":{can_reuse_recent_snapshot}}}]"),
            move || backend.get_ai_usage_snapshot(can_reuse_recent_snapshot),
        );
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.model
                    .usage
                    .finish_refresh(result.map_err(|failure| failure.reason));
                this.schedule_usage_refresh(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 결과가 만들어진 시각에서 주기를 센다. 시계가 틀어져 이미 지났으면 지금부터 센다
    fn schedule_usage_refresh(&mut self, cx: &mut Context<Self>) {
        let now_ms = cx.global::<AppServices>().clock.now_ms();
        let fetched_at_ms = self
            .model
            .usage
            .snapshot
            .as_ref()
            .and_then(|snapshot| parse_date_ms(&snapshot.fetched_at))
            .map(|fetched_at| fetched_at as i64);
        let due_ms = fetched_at_ms
            .map(|fetched_at| fetched_at + USAGE_REFRESH_INTERVAL_MS)
            .filter(|due| *due > now_ms)
            .unwrap_or(now_ms + USAGE_REFRESH_INTERVAL_MS);
        let wait = std::time::Duration::from_millis((due_ms - now_ms).max(0) as u64);
        let timer = cx.background_executor().timer(wait);
        self._usage_timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| this.refresh_usage(true, cx));
        }));
    }
}

/// 계정 추가·삭제·로그인
impl AiAccountsView {
    pub fn add_account(
        &mut self,
        provider: AiUsageProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self
            .name_inputs
            .as_ref()
            .map(|inputs| inputs[provider_index(provider)].clone())
        else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        self.submit_account_name(provider, name, Some(input), window, cx);
    }

    /// 이름을 받아 계정을 만들고 이어서 로그인 화면을 연다(`input`이 있으면 성공 뒤 비운다)
    pub fn submit_account_name(
        &mut self,
        provider: AiUsageProvider,
        name: String,
        input: Option<Entity<InputState>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if name.is_empty() || !self.model.begin_add(provider) {
            return;
        }
        let backend = self.backend.clone();
        let arguments = serde_json::json!([provider.id(), name]).to_string();
        let account_name = name.clone();
        let request = call_service(cx, "aiAccounts.addAiAccount", arguments, move || {
            backend.add_ai_account(provider, &account_name)
        });
        let window_handle = window.window_handle();
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let created = this.model.finish_add(
                        provider,
                        &name,
                        result.map_err(|failure| failure.reason),
                    );
                    if let Some(account_root) = created {
                        if let Some(input) = &input {
                            input.update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        this.load_accounts(cx);
                        this.refresh_usage(false, cx);
                        this.open_login(provider, account_root, window, cx);
                    }
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    fn retry_add(
        &mut self,
        provider: AiUsageProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let failed_name = self.model.add_states[provider_index(provider)]
            .failure
            .as_ref()
            .map(|(name, _)| name.clone());
        if let Some(name) = failed_name {
            self.submit_account_name(provider, name, None, window, cx);
        }
    }

    pub fn open_login(
        &mut self,
        provider: AiUsageProvider,
        account_root: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.open_login(provider, account_root.clone());
        let backend = self.backend.clone();
        let terminal =
            cx.new(|cx| AiAccountLoginTerminal::new(backend, provider, account_root, window, cx));
        self._login_subscription =
            Some(cx.subscribe(&terminal, |this, _, event: &LoginExited, cx| {
                if this.model.finish_login(event.0) {
                    this.after_login_closed(cx);
                }
                cx.notify();
            }));
        self.login_terminal = Some(terminal);
        cx.notify();
    }

    /// 로그인 창을 닫으면 목록과 사용량을 다시 읽는다
    pub fn close_login(&mut self, cx: &mut Context<Self>) {
        self.model.close_login();
        self.after_login_closed(cx);
        cx.notify();
    }

    fn after_login_closed(&mut self, cx: &mut Context<Self>) {
        self.login_terminal = None;
        self._login_subscription = None;
        self.load_accounts(cx);
        self.refresh_usage(false, cx);
    }

    fn confirm_remove(
        &mut self,
        account: AiAccountSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = self
            .translator("aiAccounts")
            .translate_with("removeConfirm", &[("label", &account.label)]);
        let confirm = window_confirm_prompt(window, cx);
        self.track(cx.spawn(async move |this, cx| {
            if confirm(message).await {
                let _ = this.update(cx, |this, cx| this.remove_account(account, cx));
            }
        }));
    }

    /// 한 번에 한 계정만 지운다. 실패하면 그 계정 행에 사유와 재시도를 남긴다
    pub fn remove_account(&mut self, account: AiAccountSummary, cx: &mut Context<Self>) {
        if !self.model.begin_remove(&account) {
            return;
        }
        let backend = self.backend.clone();
        let arguments =
            serde_json::json!([account.provider.id(), account.account_root]).to_string();
        let target = account.clone();
        let request = call_service(cx, "aiAccounts.removeAiAccount", arguments, move || {
            backend.remove_ai_account(target.provider, &target.account_root)
        });
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await.map(|_| ()).map_err(|failure| failure.reason);
            let _ = this.update(cx, |this, cx| {
                let is_removed = result.is_ok();
                this.model.finish_remove(account, result);
                if is_removed {
                    this.load_accounts(cx);
                    this.refresh_usage(false, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for AiAccountsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        set_window_title_once(&mut self.is_title_set, "AI Accounts", window);
        let theme = *cx.global::<ThemeTokens>();
        if self.model.accounts.is_none() {
            return self.render_first_load(&theme, cx).into_any_element();
        }
        self.ensure_name_inputs(window, cx);
        let page = self.render_page(&theme, cx);
        div()
            .size_full()
            .child(overflow_scroll_area(
                "ai-accounts-scroll",
                &self.scroll_handle,
                scrollbar_color(&theme),
                page,
            ))
            .into_any_element()
    }
}

impl AiAccountsView {
    /// provider 순서의 이름 입력란을 한 번 만든다
    fn ensure_name_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_inputs.is_some() {
            return;
        }
        let placeholder = self
            .translator("aiAccounts")
            .translate("accountNamePlaceholder");
        let inputs: [Entity<InputState>; 3] =
            std::array::from_fn(|_| create_text_field(placeholder.clone(), "", window, cx));
        // "계정 추가"는 이름이 있을 때만 눌리므로 입력이 바뀌면 다시 그린다
        self._input_subscriptions = inputs
            .iter()
            .map(|input| {
                cx.subscribe(input, |_, _, _: &gpui_kit::base::input::InputEvent, cx| {
                    cx.notify()
                })
            })
            .collect();
        self.name_inputs = Some(inputs);
    }

    /// `min-h-screen bg-bg-page p-6`, 본문 `mx-auto max-w-2xl space-y-8`
    fn render_page(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Div {
        let mut provider_cards = Vec::new();
        for provider in AiUsageProvider::ALL {
            provider_cards.push(self.render_provider_card(theme, provider, cx));
        }
        let usage_section = div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .child(self.render_usage_header(theme, cx))
            .children(self.render_usage_failure(theme))
            .children(provider_cards);
        div()
            .w_full()
            .min_h_full()
            .bg(theme.background.page)
            .p(spacing(6.0))
            .child(
                div()
                    .mx_auto()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(spacing(8.0))
                    .child(self.render_header(theme))
                    .children(self.render_refresh_failure(theme, cx))
                    .children(self.render_login_section(theme, cx))
                    .child(usage_section),
            )
    }

    fn render_first_load(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Div {
        let common = self.translator("common");
        let content: AnyElement = match self.model.load_failure.clone() {
            Some(reason) => inline_load_error(
                theme,
                "ai-accounts-load-retry",
                common.translate_with("loadFailed", &[("reason", &reason)]),
                common.translate("retry"),
                cx.listener(|this, _, _, cx| this.load_accounts(cx)),
            )
            .w_full()
            .max_w(px(448.0))
            .into_any_element(),
            None => div().child("Loading...").into_any_element(),
        };
        centered_message(theme, content).p(spacing(6.0))
    }

    fn render_header(&self, theme: &ThemeTokens) -> impl IntoElement {
        let accounts = self.translator("aiAccounts");
        let navigator = self.screen.navigator.clone();
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .child(
                        div()
                            .text_scale(text_scale::XL)
                            .css_weight(700)
                            .text_color(theme.text.primary)
                            .child(accounts.translate("title")),
                    )
                    .child(
                        div()
                            .mt(spacing(1.0))
                            .text_scale(text_scale::SM)
                            .text_color(theme.text.secondary)
                            .child(accounts.translate("description")),
                    ),
            )
            .child(text_link(
                theme,
                "ai-accounts-back",
                accounts.translate("backToSettings"),
                move |_, _, cx| navigator.navigate("/settings", cx),
            ))
    }

    /// 이미 보이던 목록을 다시 읽다 실패하면 목록 위에 사유와 재시도
    fn render_refresh_failure(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Option<Div> {
        let reason = self.model.load_failure.clone()?;
        let common = self.translator("common");
        Some(inline_load_error(
            theme,
            "ai-accounts-refresh-retry",
            common.translate_with("refreshFailed", &[("reason", &reason)]),
            common.translate("retry"),
            cx.listener(|this, _, _, cx| this.load_accounts(cx)),
        ))
    }

    /// `rounded-xl border bg-bg-surface p-5` 로그인 진행 중 카드
    fn render_login_section(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let terminal = self.login_terminal.clone()?;
        let accounts = self.translator("aiAccounts");
        let failure = self.model.login_exit_code.map(|code| {
            div()
                .mb(spacing(3.0))
                .text_scale(text_scale::XS)
                .text_color(theme.status.error)
                .child(accounts.translate_with("loginFailed", &[("code", &code.to_string())]))
        });
        Some(
            div()
                .rounded(radius::XL)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .p(spacing(5.0))
                .child(self.render_login_heading(theme, cx))
                .child(
                    div()
                        .mb(spacing(3.0))
                        .text_scale(text_scale::XS)
                        .text_color(theme.text.muted)
                        .child(accounts.translate("loginHint")),
                )
                .children(failure)
                .child(terminal),
        )
    }

    /// "로그인 진행 중" 제목과 닫기 버튼
    fn render_login_heading(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Div {
        let accounts = self.translator("aiAccounts");
        let close = make_pressable(
            outline_text_button(
                theme,
                "ai-account-login-close".into(),
                accounts.translate("closeLogin"),
                text_scale::PX_11,
            ),
            theme,
            false,
            cx.listener(|this, _, _, cx| this.close_login(cx)),
        );
        div()
            .mb(spacing(2.0))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(accounts.translate("loginInProgress")),
            )
            .child(close)
    }

    fn render_usage_header(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts = self.translator("aiAccounts");
        let usage = Translator::new(self.screen.route.locale, "taskDetail.aiUsage");
        let now_ms = cx.global::<AppServices>().clock.now_ms();
        let status = usage_block::usage_status_text(&usage, &self.model.usage, now_ms);
        let refresh = make_pressable(
            outline_text_button(
                theme,
                "ai-usage-refresh".into(),
                usage.translate("refresh"),
                text_scale::XS,
            ),
            theme,
            self.model.usage.is_refreshing,
            cx.listener(|this, _, _, cx| this.refresh_usage(false, cx)),
        );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(spacing(2.0))
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(accounts.translate("usageSection")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing(3.0))
                    .children(status.map(|text| {
                        div()
                            .text_scale(text_scale::PX_11)
                            .text_color(theme.text.muted)
                            .child(text)
                    }))
                    .child(refresh),
            )
    }

    fn render_usage_failure(&self, theme: &ThemeTokens) -> Option<impl IntoElement> {
        self.model.usage.has_failed.then(|| {
            div()
                .text_scale(text_scale::XS)
                .text_color(theme.status.error)
                .child(
                    Translator::new(self.screen.route.locale, "taskDetail.aiUsage")
                        .translate("snapshotFailed"),
                )
        })
    }

    /// `rounded-xl border bg-bg-surface p-5` provider 카드
    /// 계정 행 목록. 없으면 "등록된 계정이 없습니다."
    fn render_provider_accounts(
        &self,
        theme: &ThemeTokens,
        provider: AiUsageProvider,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let provider_accounts = self.model.accounts_of(provider);
        if provider_accounts.is_empty() {
            return div()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(self.translator("aiAccounts").translate("noAccounts"))
                .into_any_element();
        }
        let mut rows = Vec::new();
        for account in provider_accounts {
            rows.push(self.render_account_row(theme, account, cx));
        }
        div()
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .children(rows)
            .into_any_element()
    }

    fn render_provider_card(
        &self,
        theme: &ThemeTokens,
        provider: AiUsageProvider,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body = self.render_provider_accounts(theme, provider, cx);
        div()
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .p(spacing(5.0))
            .child(
                div()
                    .mb(spacing(3.0))
                    .flex()
                    .items_center()
                    .gap(spacing(2.0))
                    .child(usage_block::render_provider_icon(theme, provider))
                    .child(
                        div()
                            .text_scale(text_scale::SM)
                            .css_weight(600)
                            .text_color(theme.text.primary)
                            .child(provider_title(provider)),
                    ),
            )
            .child(body)
            .child(self.render_add_form(theme, provider, cx))
            .into_any_element()
    }
}

mod rows;

/// 테두리만 있는 작은 글자 버튼(`rounded-md border px-2 py-1 text-text-secondary hover:text-text-primary`)
pub(crate) fn outline_text_button(
    theme: &ThemeTokens,
    id: gpui_kit::ElementId,
    label: String,
    scale: crate::theme::TextScale,
) -> gpui_kit::Stateful<Div> {
    let hover = theme.text.primary;
    pressable(id, "button", label.clone(), label.clone())
        .flex_none()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(scale)
        .text_color(theme.text.secondary)
        .hover(move |style| style.text_color(hover))
        .child(label)
}
