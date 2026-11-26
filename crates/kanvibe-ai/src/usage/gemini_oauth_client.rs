//! Gemini 토큰 갱신에 필요한 OAuth client. 그 값이 Gemini CLI 번들 안에만 있어 설치본을 찾아 읽는다.
//! 설치본이 없거나 번들 형식이 바뀌면 갱신을 포기하고 재로그인을 안내한다.

use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::sync::LazyLock;

use crate::javascript::{JS_WHITESPACE_CLASS, quote_json_string};
use crate::node_path;
use crate::usage::context::{HttpRequest, UsageContext};
use crate::usage::shared::AI_USAGE_REQUEST_TIMEOUT_MS;

pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const LOAD_CODE_ASSIST_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";

/// Gemini CLI가 OAuth client 자격을 심어 두는 번들 파일. 설치 방식마다 루트만 다르고 이 아래 경로는 같다
const OAUTH2_BUNDLE_SUBPATH: &str =
    "node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js";

/// npm 전역·pnpm·Homebrew 설치본에서 번들이 바이너리 기준 몇 단계 위에 있는지의 관측 범위
const BUNDLE_SEARCH_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeminiOAuthClient {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeminiRefreshedToken {
    pub access_token: String,
    pub expires_at: f64,
}

/// PATH를 직접 훑고, 그다음 흔한 설치 위치를 본다. GUI로 실행된 앱은 로그인 셸의 PATH를 물려받지 못할 수 있다
fn find_gemini_binary(context: &UsageContext) -> Option<String> {
    let path_directories = context
        .variable("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|directory| !directory.is_empty())
        .map(str::to_string);
    let home = context.home_directory.as_str();
    let fallback_directories = context.system_binary_directories.iter().cloned().chain([
        node_path::join(&[home, ".local", "bin"]),
        node_path::join(&[home, "bin"]),
    ]);
    let candidate = path_directories
        .chain(fallback_directories)
        .map(|directory| node_path::join(&[&directory, "gemini"]))
        .find(|candidate| Path::new(candidate).exists())?;
    // Homebrew처럼 bin이 심볼릭 링크면 링크를 풀어야 실제 설치 루트가 나온다
    Some(
        std::fs::canonicalize(&candidate)
            .map(|resolved| resolved.to_string_lossy().into_owned())
            .unwrap_or(candidate),
    )
}

fn find_oauth_bundle(gemini_binary_path: &str) -> Option<String> {
    let mut ancestor = node_path::dirname(&node_path::dirname(gemini_binary_path));
    for _ in 0..BUNDLE_SEARCH_DEPTH {
        let candidate = node_path::join(&[&ancestor, OAUTH2_BUNDLE_SUBPATH]);
        if Path::new(&candidate).exists() {
            return Some(candidate);
        }
        let parent = node_path::dirname(&ancestor);
        if parent == ancestor {
            break;
        }
        ancestor = parent;
    }
    None
}

static CLIENT_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"OAUTH_CLIENT_ID[{JS_WHITESPACE_CLASS}]*=[{JS_WHITESPACE_CLASS}]*['"]([^'"]+)['"]"#
    ))
    .expect("client id 패턴")
});

static CLIENT_SECRET_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"OAUTH_CLIENT_SECRET[{JS_WHITESPACE_CLASS}]*=[{JS_WHITESPACE_CLASS}]*['"]([^'"]+)['"]"#
    ))
    .expect("client secret 패턴")
});

fn parse_oauth_client(bundle_source: &str) -> Option<GeminiOAuthClient> {
    let client_id = CLIENT_ID_PATTERN.captures(bundle_source)?[1].to_string();
    let client_secret = CLIENT_SECRET_PATTERN.captures(bundle_source)?[1].to_string();
    Some(GeminiOAuthClient {
        client_id,
        client_secret,
    })
}

/// Gemini는 만료된 액세스 토큰을 갱신할 때 CLI가 쓰는 것과 같은 OAuth client를 요구한다
pub fn resolve_gemini_oauth_client(context: &UsageContext) -> Option<GeminiOAuthClient> {
    let gemini_binary_path = find_gemini_binary(context)?;
    let bundle_path = find_oauth_bundle(&gemini_binary_path)?;
    let bundle_source = std::fs::read(bundle_path).ok()?;
    parse_oauth_client(&String::from_utf8_lossy(&bundle_source))
}

/// JS `URLSearchParams.toString()`의 application/x-www-form-urlencoded 인코딩
fn form_urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

pub fn refresh_gemini_access_token(
    context: &UsageContext,
    refresh_token: &str,
    client: &GeminiOAuthClient,
) -> Option<GeminiRefreshedToken> {
    let body = [
        ("client_id", client.client_id.as_str()),
        ("client_secret", client.client_secret.as_str()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ]
    .iter()
    .map(|(name, value)| format!("{}={}", form_urlencode(name), form_urlencode(value)))
    .collect::<Vec<_>>()
    .join("&");
    let response = context
        .http
        .send(&HttpRequest {
            method: "POST".to_string(),
            url: GOOGLE_TOKEN_URL.to_string(),
            headers: vec![(
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            )],
            body: Some(body),
            timeout_ms: AI_USAGE_REQUEST_TIMEOUT_MS,
        })
        .ok()
        .filter(|response| response.is_ok())?;
    let payload = response.json()?;
    let access_token = payload
        .get("access_token")?
        .as_str()
        .filter(|token| !token.is_empty())?;
    let expires_in_seconds = payload
        .get("expires_in")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    Some(GeminiRefreshedToken {
        access_token: access_token.to_string(),
        expires_at: context.now_ms() + expires_in_seconds * 1000.0,
    })
}

/// 쿼터 조회는 프로젝트 단위라 액세스 토큰만으로는 부족하고 이 호출로 프로젝트를 먼저 알아내야 한다
pub fn load_gemini_project_id(context: &UsageContext, access_token: &str) -> Option<String> {
    let response = context
        .http
        .send(&HttpRequest {
            method: "POST".to_string(),
            url: LOAD_CODE_ASSIST_URL.to_string(),
            headers: vec![
                (
                    "Authorization".to_string(),
                    format!("Bearer {access_token}"),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: Some("{}".to_string()),
            timeout_ms: AI_USAGE_REQUEST_TIMEOUT_MS,
        })
        .ok()
        .filter(|response| response.is_ok())?;
    let project = response
        .json()?
        .get("cloudaicompanionProject")?
        .as_str()?
        .to_string();
    (!project.is_empty()).then_some(project)
}

/// 쿼터 조회 본문(TS `JSON.stringify({ project })`)
pub fn project_request_body(project_id: &str) -> String {
    format!("{{\"project\":{}}}", quote_json_string(project_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{temp_home, write_file};
    use crate::usage::context::UsagePlatform;
    use crate::usage::test_context::TestUsage;

    /// `<임시>/usr/local/bin/gemini`와 그 설치본 번들을 흉내 낸다
    fn install_root() -> (tempfile::TempDir, String, TestUsage) {
        let (directory, root) = temp_home();
        let path_value = format!("{root}/usr/local/bin:{root}/usr/bin");
        let usage = TestUsage::with(
            &format!("{root}/home"),
            UsagePlatform::Linux,
            &[("PATH", &path_value)],
        );
        (directory, root, usage)
    }

    fn bundle_path(root: &str) -> String {
        format!("{root}/usr/local/{OAUTH2_BUNDLE_SUBPATH}")
    }

    #[test]
    fn missing_binary_is_none() {
        let (_directory, _root, usage) = install_root();
        assert_eq!(resolve_gemini_oauth_client(&usage.context), None);
    }

    #[test]
    fn extracts_client_from_installed_bundle() {
        let (_directory, root, usage) = install_root();
        write_file(&format!("{root}/usr/local/bin/gemini"), "");
        write_file(
            &bundle_path(&root),
            "const OAUTH_CLIENT_ID = \"bundle-client-id.apps.googleusercontent.com\";\nconst OAUTH_CLIENT_SECRET = 'bundle-client-secret';\n",
        );
        assert_eq!(
            resolve_gemini_oauth_client(&usage.context),
            Some(GeminiOAuthClient {
                client_id: "bundle-client-id.apps.googleusercontent.com".to_string(),
                client_secret: "bundle-client-secret".to_string(),
            })
        );
    }

    #[test]
    fn changed_bundle_format_is_none() {
        let (_directory, root, usage) = install_root();
        write_file(&format!("{root}/usr/local/bin/gemini"), "");
        write_file(
            &bundle_path(&root),
            "export const clientConfig = loadFromVault();",
        );
        assert_eq!(resolve_gemini_oauth_client(&usage.context), None);
    }

    #[test]
    fn refresh_posts_form_encoded_body() {
        let usage = TestUsage::new("/home/tester");
        usage.http.respond_json(
            200,
            serde_json::json!({"access_token": "fresh", "expires_in": 60}),
        );
        let client = GeminiOAuthClient {
            client_id: "id 1".to_string(),
            client_secret: "s/=".to_string(),
        };
        let refreshed = refresh_gemini_access_token(&usage.context, "r&t", &client).unwrap();
        assert_eq!(refreshed.access_token, "fresh");
        assert_eq!(refreshed.expires_at, usage.context.now_ms() + 60_000.0);
        assert_eq!(
            usage.http.request(0).body.as_deref(),
            Some(
                "client_id=id+1&client_secret=s%2F%3D&refresh_token=r%26t&grant_type=refresh_token"
            )
        );
    }
}
