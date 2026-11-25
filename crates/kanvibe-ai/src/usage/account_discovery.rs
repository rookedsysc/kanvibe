//! provider마다 로그인된 계정을 찾는다. 환경 변수로 지정한 루트, 홈의 기본 루트, 접두사가 같은 형제 루트,
//! KanVibe가 만든 계정 순으로 후보를 모으고 자격증명이 있는 것만 계정으로 센다.

use base64::Engine;
use serde_json::{Map, Value};

use crate::javascript::{js_trim, parse_json};
use crate::node_path;
use crate::usage::account_registry::AiAccountRegistration;
use crate::usage::claude_credentials::{
    ClaudeKeychainReadResult, read_claude_access_token, read_claude_keychain_credentials,
};
use crate::usage::context::UsageContext;
use crate::usage::provider_config_dir::{
    AiProviderConfigDirSpec, config_dir_spec, to_account_config_dir, to_account_name_from_root,
    to_default_account_root,
};
use crate::usage::types::{AiUsageAccount, AiUsageProvider};

#[derive(Debug, Clone, Default)]
struct ProviderAccountIdentity {
    account_id: Option<String>,
    label: Option<String>,
}

/// Node `fs.readdir`는 libuv scandir가 이름을 바이트 순서로 정렬해 돌려주므로 같은 순서로 맞춘다
fn sorted_home_directories(home_directory: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(home_directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();
    names
}

/// 조회를 시도할 계정 루트 후보를 신뢰 순서대로 모은다
fn collect_account_root_candidates(
    context: &UsageContext,
    spec: &AiProviderConfigDirSpec,
    registrations: &[AiAccountRegistration],
) -> Vec<String> {
    let home_directory = context.home_directory.as_str();
    let configured_root = context
        .variable(spec.home_env_var)
        .map(js_trim)
        .filter(|root| !root.is_empty())
        .map(|root| node_path::resolve(&[root]));
    let sibling_roots = sorted_home_directories(home_directory)
        .into_iter()
        .filter(|name| name.starts_with(spec.sibling_root_prefix))
        .map(|name| node_path::join(&[home_directory, &name]));
    let candidates = configured_root
        .into_iter()
        .chain([to_default_account_root(spec, home_directory)])
        .chain(sibling_roots)
        .chain(
            registrations
                .iter()
                .map(|registration| registration.account_root.clone()),
        );
    let mut unique: Vec<String> = Vec::new();
    for candidate in candidates {
        if !unique.contains(&candidate) {
            unique.push(candidate);
        }
    }
    unique
}

fn read_json_file(file_path: &str) -> Option<Map<String, Value>> {
    let bytes = std::fs::read(file_path).ok()?;
    match parse_json(&String::from_utf8_lossy(&bytes))? {
        Value::Object(record) => Some(record),
        _ => None,
    }
}

fn first_non_empty_string(values: &[Option<&Value>]) -> Option<String> {
    values.iter().find_map(|value| {
        let trimmed = js_trim(value.and_then(Value::as_str)?);
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

/// 설정 파일이 로그인한 계정을 알려줄 때만 신원으로 친다. 둘 다 비면 이 파일은 출처가 되지 못한다
fn read_claude_config_identity(
    config: Option<&Map<String, Value>>,
) -> Option<ProviderAccountIdentity> {
    let oauth_account = config.and_then(|config| config.get("oauthAccount"));
    let field = |name: &str| oauth_account.and_then(|account| account.get(name));
    let identity = ProviderAccountIdentity {
        account_id: first_non_empty_string(&[field("accountUuid")]),
        label: first_non_empty_string(&[field("emailAddress"), field("displayName")]),
    };
    (identity.account_id.is_some() || identity.label.is_some()).then_some(identity)
}

/// Claude는 사용량 응답에 계정 정보가 없어 `.claude.json`이 유일한 식별·라벨 출처다. config dir 안의 파일이 계정을 알려주지
/// 않을 때 홈으로 넘어가는 것은 기본 계정뿐이다. 형제 루트까지 넘기면 다른 계정의 신원을 물려받아 합쳐진다
fn read_claude_account_identity(
    context: &UsageContext,
    config_dir: &str,
) -> ProviderAccountIdentity {
    let colocated = read_json_file(&node_path::join(&[config_dir, ".claude.json"]));
    if let Some(identity) = read_claude_config_identity(colocated.as_ref()) {
        return identity;
    }
    let default_root = to_default_account_root(
        config_dir_spec(AiUsageProvider::Claude),
        &context.home_directory,
    );
    if config_dir != default_root {
        return ProviderAccountIdentity::default();
    }
    let home_config = read_json_file(&node_path::join(&[&context.home_directory, ".claude.json"]));
    read_claude_config_identity(home_config.as_ref()).unwrap_or_default()
}

fn read_codex_account_identity(config_dir: &str) -> ProviderAccountIdentity {
    let auth = read_json_file(&node_path::join(&[config_dir, "auth.json"]));
    let tokens = auth.as_ref().and_then(|auth| auth.get("tokens"));
    ProviderAccountIdentity {
        account_id: first_non_empty_string(&[tokens.and_then(|tokens| tokens.get("account_id"))]),
        label: read_email_from_id_token(tokens.and_then(|tokens| tokens.get("id_token"))),
    }
}

/// Gemini CLI는 계정 이메일을 남기지 않는 판이 있어, 계정을 가를 안정적인 로컬 값이 경로뿐이다
fn read_account_identity(
    context: &UsageContext,
    provider: AiUsageProvider,
    config_dir: &str,
) -> ProviderAccountIdentity {
    match provider {
        AiUsageProvider::Claude => read_claude_account_identity(context, config_dir),
        AiUsageProvider::Codex => read_codex_account_identity(config_dir),
        AiUsageProvider::Gemini => ProviderAccountIdentity::default(),
    }
}

/// provider마다 자격증명 파일에서 로그인을 증명하는 자리가 다르다
fn read_stored_access_token(
    provider: AiUsageProvider,
    credentials: &Map<String, Value>,
) -> Option<String> {
    let token = match provider {
        AiUsageProvider::Claude => credentials
            .get("claudeAiOauth")
            .and_then(|oauth| oauth.get("accessToken")),
        AiUsageProvider::Codex => credentials
            .get("tokens")
            .and_then(|tokens| tokens.get("access_token")),
        AiUsageProvider::Gemini => credentials.get("access_token"),
    };
    first_non_empty_string(&[token])
}

/// 이 디렉터리에 로그인을 증명하는 자격증명이 들어 있는지
pub fn has_stored_credentials(spec: &AiProviderConfigDirSpec, config_dir: &str) -> bool {
    read_json_file(&node_path::join(&[config_dir, spec.credentials_file_name]))
        .is_some_and(|credentials| read_stored_access_token(spec.provider, &credentials).is_some())
}

/// Node `Buffer.from(text, "base64url")`처럼 두 알파벳을 모두 받고 패딩과 낯선 글자는 건너뛴다
fn decode_lenient_base64(text: &str) -> Vec<u8> {
    let normalized: String = text
        .chars()
        .filter_map(|character| match character {
            '+' => Some('-'),
            '/' => Some('_'),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' => Some(character),
            _ => None,
        })
        .collect();
    let usable_length = normalized.len() - usize::from(normalized.len() % 4 == 1);
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::URL_SAFE,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    engine
        .decode(&normalized[..usable_length])
        .unwrap_or_default()
}

/// id_token은 서명 검증 없이 표시용 이메일만 꺼낸다. 로컬 파일이고 인증에 쓰지 않는다
fn read_email_from_id_token(id_token: Option<&Value>) -> Option<String> {
    let payload_segment = id_token?
        .as_str()?
        .split('.')
        .nth(1)
        .filter(|segment| !segment.is_empty())?;
    let payload = parse_json(&String::from_utf8_lossy(&decode_lenient_base64(
        payload_segment,
    )))?;
    first_non_empty_string(&[payload.get("email")])
}

/// 같은 계정이 여러 경로에서 발견되면 가장 신뢰도 높은 첫 후보만 남긴다
fn deduplicate_accounts(accounts: Vec<AiUsageAccount>) -> Vec<AiUsageAccount> {
    let mut seen_account_ids: Vec<String> = Vec::new();
    accounts
        .into_iter()
        .filter(|account| {
            let is_new = !seen_account_ids.contains(&account.account_id);
            seen_account_ids.push(account.account_id.clone());
            is_new
        })
        .collect()
}

/// macOS의 Claude Code는 자격증명을 Keychain에 두므로 config dir에 파일이 없어도 로그인되어 있을 수 있다.
/// Keychain을 읽지 못한 경우에도 계정 자리를 만들어, 조회 단계가 그 사유를 화면에 알리게 한다
fn discover_claude_keychain_account(
    context: &UsageContext,
    config_dirs: &[String],
) -> Option<AiUsageAccount> {
    let spec = config_dir_spec(AiUsageProvider::Claude);
    let config_dir = to_default_account_root(spec, &context.home_directory);
    let has_account = match read_claude_keychain_credentials(context, config_dirs) {
        ClaudeKeychainReadResult::Unreadable => true,
        ClaudeKeychainReadResult::Found { credentials } => {
            read_claude_access_token(&credentials).is_some()
        }
        ClaudeKeychainReadResult::Absent => false,
    };
    if !has_account {
        return None;
    }
    let identity = read_claude_account_identity(context, &config_dir);
    Some(AiUsageAccount {
        provider: AiUsageProvider::Claude,
        account_id: identity.account_id.unwrap_or_else(|| config_dir.clone()),
        label: identity
            .label
            .unwrap_or_else(|| spec.default_label.to_string()),
        config_dir: config_dir.clone(),
        account_root: config_dir,
    })
}

fn to_account_label(
    spec: &AiProviderConfigDirSpec,
    identity: &ProviderAccountIdentity,
    account_root: &str,
) -> String {
    identity
        .label
        .clone()
        .or_else(|| to_account_name_from_root(spec, account_root))
        .unwrap_or_else(|| spec.default_label.to_string())
}

/// provider 하나의 계정을 모두 찾는다. KanVibe에서 만든 계정은 로그아웃돼 있어도 목록에 남겨 다시 로그인할 자리를 지킨다
pub fn discover_provider_accounts(
    context: &UsageContext,
    provider: AiUsageProvider,
    registrations: &[AiAccountRegistration],
) -> Vec<AiUsageAccount> {
    let spec = config_dir_spec(provider);
    let provider_registrations: Vec<AiAccountRegistration> = registrations
        .iter()
        .filter(|registration| registration.provider == provider)
        .cloned()
        .collect();
    let candidate_roots = collect_account_root_candidates(context, spec, &provider_registrations);
    let mut accounts: Vec<AiUsageAccount> = candidate_roots
        .iter()
        .filter_map(|account_root| {
            discover_root_account(context, spec, &provider_registrations, account_root)
        })
        .collect();
    // 기본 계정을 파일로 찾았으면 Keychain 승인 프롬프트를 띄울 이유가 없다
    let default_root = to_default_account_root(spec, &context.home_directory);
    let has_default_account = accounts
        .iter()
        .any(|account| account.account_root == default_root);
    if provider == AiUsageProvider::Claude && !has_default_account {
        accounts.extend(discover_claude_keychain_account(context, &candidate_roots));
    }
    deduplicate_accounts(accounts)
}

fn discover_root_account(
    context: &UsageContext,
    spec: &AiProviderConfigDirSpec,
    registrations: &[AiAccountRegistration],
    account_root: &str,
) -> Option<AiUsageAccount> {
    let config_dir = to_account_config_dir(spec, account_root);
    let is_registered = registrations
        .iter()
        .any(|registration| registration.account_root == account_root);
    if !is_registered && !has_stored_credentials(spec, &config_dir) {
        return None;
    }
    let identity = read_account_identity(context, spec.provider, &config_dir);
    Some(AiUsageAccount {
        provider: spec.provider,
        account_id: identity
            .account_id
            .clone()
            .unwrap_or_else(|| account_root.to_string()),
        label: to_account_label(spec, &identity, account_root),
        config_dir,
        account_root: account_root.to_string(),
    })
}

#[cfg(test)]
mod tests;
