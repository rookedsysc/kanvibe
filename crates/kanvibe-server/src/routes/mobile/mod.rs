//! 모바일 HTTP 경로(TS `electron/mobileRoutes.js` `handleMobileRequest`).
//!
//! 순서가 응답을 정한다: 개발용 페어링 코드 → 페어링 → 기기 인증 → 기능 경로(다이어그램, AI로 이어짐) → 기본 경로 → 404.
//! 기능 경로가 먼저라 `tasks/search`·`tasks/done`은 예약된 태스크 ID이고, `POST …/notifications/read`는 기능 경로의
//! `updatedCount`로 답한다(기본 경로의 `count` 처리기는 닿지 않는다).

mod ai;
mod ai_history;
mod diagram;
mod feature;
mod main_routes;
mod pane_chat;
mod tasks;

use http::Method;
use serde_json::{Value, json};

use super::js_semantics::{MobileUrl, UriError, property};
use crate::bridge::{BridgeError, BridgeResult, MobileBridge};
use crate::http::HttpRequest;
use crate::http::body::{
    ConnectionAbort, HttpResponse, MAX_BODY_BYTES, failure_response, json_response,
};
use crate::server::ServerState;

pub const MOBILE_PATH_PREFIX: &str = "/api/mobile/";
const DEV_PAIRING_CODE_PATH: &str = "/api/mobile/dev/pairing-code";
/// 목록에서 기기를 알아보기 위한 이름이라 이 길이(UTF-16 단위)로 자른다
const MAX_DEVICE_NAME_LENGTH: usize = 64;
pub const UNPAIRED_DEVICE_ERROR: &str = "연결되지 않은 기기입니다";
pub const BAD_REQUEST_ERROR: &str = "잘못된 요청입니다";
pub const SERVER_ERROR: &str = "서버 오류";

/// 라우트 처리 중 응답을 직접 고르지 못한 실패. 바깥 처리기가 TS의 전역 catch처럼 응답을 정한다
#[derive(Debug)]
pub enum RouteError {
    Abort,
    Bridge(BridgeError),
    /// `URIError`, `TypeError` 같은 이름 없는 예외 → 500
    Unexpected,
}

impl From<ConnectionAbort> for RouteError {
    fn from(_: ConnectionAbort) -> Self {
        Self::Abort
    }
}

impl From<BridgeError> for RouteError {
    fn from(error: BridgeError) -> Self {
        Self::Bridge(error)
    }
}

impl From<UriError> for RouteError {
    fn from(_: UriError) -> Self {
        Self::Unexpected
    }
}

pub type RouteResult = Result<Option<HttpResponse>, RouteError>;

/// 인증을 통과한 모바일 요청
pub struct MobileRequest<'a> {
    pub request: HttpRequest,
    pub url: MobileUrl,
    pub device_id: String,
    pub state: &'a ServerState,
}

impl MobileRequest<'_> {
    pub fn is(&self, method: Method, pathname: &str) -> bool {
        self.request.method == method && self.url.pathname == pathname
    }

    pub fn method_is(&self, method: &Method) -> bool {
        self.request.method == *method
    }

    /// 기본 한도로 JSON 본문을 읽는다. 읽지 못하면 `None`
    pub async fn read_json(&mut self, max_bytes: usize) -> Result<Option<Value>, RouteError> {
        Ok(self.request.body.read_json(max_bytes).await?.ok())
    }

    pub async fn read_default_json(&mut self) -> Result<Option<Value>, RouteError> {
        self.read_json(MAX_BODY_BYTES).await
    }

    pub async fn bridge<T: Send + 'static>(
        &self,
        call: impl FnOnce(&dyn MobileBridge) -> BridgeResult<T> + Send + 'static,
    ) -> BridgeResult<T> {
        call_bridge(self.state, call).await
    }
}

/// 블로킹 서비스를 tokio 작업 스레드 밖에서 부른다
pub async fn call_bridge<T: Send + 'static>(
    state: &ServerState,
    call: impl FnOnce(&dyn MobileBridge) -> BridgeResult<T> + Send + 'static,
) -> BridgeResult<T> {
    let bridge = state.bridge.clone();
    tokio::task::spawn_blocking(move || call(bridge.as_ref()))
        .await
        .unwrap_or_else(|error| Err(BridgeError::Internal(error.to_string())))
}

/// 모바일 경로가 아니면 `None`을 돌려 hook 라우팅이 이어가게 한다
pub async fn handle_mobile_request(
    request: HttpRequest,
    state: &ServerState,
) -> Result<Option<HttpResponse>, ConnectionAbort> {
    if !request.raw_url.starts_with(MOBILE_PATH_PREFIX) {
        return Ok(None);
    }
    match dispatch_mobile_request(request, state).await {
        Ok(response) => Ok(Some(response)),
        Err(RouteError::Abort) => Err(ConnectionAbort),
        Err(RouteError::Bridge(error)) => Ok(Some(global_failure_response(&error))),
        Err(RouteError::Unexpected) => Ok(Some(failure_response(500, SERVER_ERROR))),
    }
}

/// TS 전역 catch: `MobileFeatureError` 400·404·409와 `MobileTaskInputError`만 메시지를 내보낸다
pub fn global_failure_response(error: &BridgeError) -> HttpResponse {
    match error {
        BridgeError::Feature { status, message } if matches!(status, 400 | 404 | 409) => {
            failure_response(*status, message)
        }
        BridgeError::TaskInput(message) => failure_response(400, message),
        _ => failure_response(500, SERVER_ERROR),
    }
}

async fn dispatch_mobile_request(
    request: HttpRequest,
    state: &ServerState,
) -> Result<HttpResponse, RouteError> {
    let url = MobileUrl::parse(&request.raw_url).ok_or(RouteError::Unexpected)?;
    let is_post = request.method == Method::POST;
    if is_post
        && url.pathname == DEV_PAIRING_CODE_PATH
        && state.options.is_dev_pairing_route_enabled
        && request.peer.is_loopback()
    {
        let code = call_bridge(state, |bridge| bridge.start_mobile_pairing()).await?;
        return Ok(json_response(200, &spread_success(code)));
    }
    if is_post && url.pathname == "/api/mobile/pair" {
        return handle_pair_request(request, state).await;
    }
    let authorization = request.authorization().map(str::to_string);
    let device_id = call_bridge(state, move |bridge| {
        bridge.authorize_mobile_request(authorization.as_deref())
    })
    .await?;
    let Some(device_id) = device_id else {
        return Ok(failure_response(401, UNPAIRED_DEVICE_ERROR));
    };
    let mut mobile = MobileRequest {
        request,
        url,
        device_id,
        state,
    };
    if let Some(response) = feature::handle_feature_request(&mut mobile).await? {
        return Ok(response);
    }
    main_routes::handle_main_routes(&mut mobile).await
}

async fn handle_pair_request(
    mut request: HttpRequest,
    state: &ServerState,
) -> Result<HttpResponse, RouteError> {
    let Ok(body) = request.body.read_json(MAX_BODY_BYTES).await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    let code = match property(&body, "code").map_err(|_| RouteError::Unexpected)? {
        Some(Value::String(code)) => code.clone(),
        _ => String::new(),
    };
    let submitted_name = match property(&body, "deviceName").map_err(|_| RouteError::Unexpected)? {
        Some(Value::String(name)) => {
            kanvibe_ai::javascript::utf16_prefix(name, MAX_DEVICE_NAME_LENGTH).to_string()
        }
        _ => String::new(),
    };
    let device_name = if submitted_name.is_empty() {
        "모바일 기기".to_string()
    } else {
        submitted_name
    };
    let token = call_bridge(state, move |bridge| {
        bridge.pair_mobile_device(&code, &device_name)
    })
    .await?;
    Ok(match token {
        Some(token) => json_response(200, &json!({ "success": true, "token": token })),
        None => failure_response(401, "코드가 맞지 않거나 만료되었습니다"),
    })
}

/// `{ success: true, ...value }`. 객체가 아닌 값은 펼칠 칸이 없다
pub fn spread_success(value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("success".to_string(), Value::Bool(true));
    if let Value::Object(fields) = value {
        payload.extend(fields);
    }
    Value::Object(payload)
}

/// `{ success: true, <name>: value }`
pub fn success_with(name: &str, value: Value) -> HttpResponse {
    let mut payload = serde_json::Map::new();
    payload.insert("success".to_string(), Value::Bool(true));
    payload.insert(name.to_string(), value);
    json_response(200, &Value::Object(payload))
}
