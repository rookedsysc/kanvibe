//! 실행 인자. 런타임 배선은 프로세스 환경 변수 대신 명시적 인자로만 받는다(CLAUDE.md Runtime Environment Safety).

use std::path::PathBuf;

/// QA 모드(`--qa`)에서만 쓰는 결정성 장치
#[derive(Debug, Clone, PartialEq)]
pub struct QaOptions {
    /// fixture 루트. 단계 안 `{fixture_root}` 치환과 정리 기준이다
    pub fixture_root: Option<PathBuf>,
    /// 앱이 보는 현재 시각의 출발점(epoch ms). 이후에는 실제 경과 시간만큼만 흐른다
    pub fixed_now_ms: i64,
    /// 바깥 HTTP 요청에 돌려줄 고정 응답 표(`parity/fixtures/network/*.json`)
    pub network_responses_path: Option<PathBuf>,
    /// 첫 화면 전에 걸 서비스 고장 목록(JSON 배열)
    pub faults_path: Option<PathBuf>,
    /// 재생 드라이버가 명령을 보내는 Unix 소켓
    pub control_socket_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOptions {
    /// `kanvibe.db`가 놓인 앱 데이터 디렉터리
    pub data_dir: Option<PathBuf>,
    /// 첫 창이 여는 `#` 뒤 경로
    pub initial_route: String,
    /// 첫 창의 크기(논리 px)
    pub window_size: (f32, f32),
    pub qa: Option<QaOptions>,
}

pub const DEFAULT_WINDOW_SIZE: (f32, f32) = (1600.0, 1000.0);
const DEFAULT_ROUTE: &str = "/ko";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchOptionsError {
    #[error("알 수 없는 인자입니다: {0}")]
    UnknownArgument(String),
    #[error("{0} 뒤에 값이 필요합니다")]
    MissingValue(String),
    #[error("{flag} 값을 읽지 못했습니다: {value}")]
    InvalidValue { flag: String, value: String },
    #[error("--qa에는 --fixed-now가 필요합니다")]
    MissingFixedNow,
    #[error("{0}는 --qa와 함께만 쓸 수 있습니다")]
    QaOnly(String),
}

#[derive(Default)]
struct RawLaunchArguments {
    is_qa: bool,
    data_dir: Option<PathBuf>,
    route: Option<String>,
    window_size: Option<(f32, f32)>,
    fixture_root: Option<PathBuf>,
    fixed_now_ms: Option<i64>,
    network_responses_path: Option<PathBuf>,
    faults_path: Option<PathBuf>,
    control_socket_path: Option<PathBuf>,
}

/// 실행 파일 이름을 뺀 인자 목록을 읽는다
pub fn parse_launch_options(
    arguments: impl IntoIterator<Item = String>,
) -> Result<LaunchOptions, LaunchOptionsError> {
    let mut raw = RawLaunchArguments::default();
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        if flag == "--qa" {
            raw.is_qa = true;
            continue;
        }
        let value = arguments
            .next()
            .ok_or_else(|| LaunchOptionsError::MissingValue(flag.clone()))?;
        apply_flag(&mut raw, &flag, value)?;
    }
    build_launch_options(raw)
}

fn apply_flag(
    raw: &mut RawLaunchArguments,
    flag: &str,
    value: String,
) -> Result<(), LaunchOptionsError> {
    let invalid = |value: &str| LaunchOptionsError::InvalidValue {
        flag: flag.to_string(),
        value: value.to_string(),
    };
    match flag {
        "--data-dir" => raw.data_dir = Some(PathBuf::from(value)),
        "--route" => raw.route = Some(value),
        "--window-size" => {
            raw.window_size = Some(parse_window_size(&value).ok_or(invalid(&value))?)
        }
        "--fixture-root" => raw.fixture_root = Some(PathBuf::from(value)),
        "--fixed-now" => raw.fixed_now_ms = Some(parse_iso_utc_ms(&value).ok_or(invalid(&value))?),
        "--network-responses" => raw.network_responses_path = Some(PathBuf::from(value)),
        "--faults" => raw.faults_path = Some(PathBuf::from(value)),
        "--control-socket" => raw.control_socket_path = Some(PathBuf::from(value)),
        unknown => return Err(LaunchOptionsError::UnknownArgument(unknown.to_string())),
    }
    Ok(())
}

fn build_launch_options(raw: RawLaunchArguments) -> Result<LaunchOptions, LaunchOptionsError> {
    let qa = if raw.is_qa {
        Some(QaOptions {
            fixture_root: raw.fixture_root,
            fixed_now_ms: raw
                .fixed_now_ms
                .ok_or(LaunchOptionsError::MissingFixedNow)?,
            network_responses_path: raw.network_responses_path,
            faults_path: raw.faults_path,
            control_socket_path: raw.control_socket_path,
        })
    } else {
        reject_qa_only_flags(&raw)?;
        None
    };
    Ok(LaunchOptions {
        data_dir: raw.data_dir,
        initial_route: raw.route.unwrap_or_else(|| DEFAULT_ROUTE.to_string()),
        window_size: raw.window_size.unwrap_or(DEFAULT_WINDOW_SIZE),
        qa,
    })
}

/// 결정성 장치가 일반 실행에 섞이면 사용자 화면이 고정 시각·가짜 응답으로 움직이므로 거부한다
fn reject_qa_only_flags(raw: &RawLaunchArguments) -> Result<(), LaunchOptionsError> {
    let qa_only_flags = [
        ("--fixture-root", raw.fixture_root.is_some()),
        ("--fixed-now", raw.fixed_now_ms.is_some()),
        ("--network-responses", raw.network_responses_path.is_some()),
        ("--faults", raw.faults_path.is_some()),
        ("--control-socket", raw.control_socket_path.is_some()),
    ];
    match qa_only_flags.into_iter().find(|(_, is_set)| *is_set) {
        Some((flag, _)) => Err(LaunchOptionsError::QaOnly(flag.to_string())),
        None => Ok(()),
    }
}

fn parse_window_size(value: &str) -> Option<(f32, f32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// `2026-01-15T03:00:00Z`, `2026-01-15T03:00:00.250Z` 형식의 UTC 시각을 epoch ms로 읽는다
pub fn parse_iso_utc_ms(value: &str) -> Option<i64> {
    let text = value.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date_parts = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (
        date_parts.next()?.ok()?,
        date_parts.next()?.ok()?,
        date_parts.next()?.ok()?,
    );
    let (clock, fraction) = time.split_once('.').unwrap_or((time, "0"));
    let mut clock_parts = clock.splitn(3, ':').map(str::parse::<i64>);
    let (hour, minute, second) = (
        clock_parts.next()?.ok()?,
        clock_parts.next()?.ok()?,
        clock_parts.next()?.ok()?,
    );
    let millis: i64 = format!("{fraction:0<3}")[..3].parse().ok()?;
    let is_valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61;
    is_valid.then(|| {
        let days = days_from_civil(year, month, day);
        ((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000 + millis
    })
}

/// 그레고리력 날짜의 1970-01-01부터 날 수(Howard Hinnant의 days_from_civil)
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn parses_the_parity_fixed_clock() {
        assert_eq!(
            parse_iso_utc_ms("2026-01-15T03:00:00Z"),
            Some(1_768_446_000_000)
        );
        assert_eq!(parse_iso_utc_ms("1970-01-01T00:00:00.250Z"), Some(250));
        assert_eq!(parse_iso_utc_ms("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_iso_utc_ms("2026-01-15T03:00:00"), None);
    }

    #[test]
    fn qa_mode_reads_every_determinism_input_from_arguments() {
        let options = parse_launch_options(arguments(
            "--qa --data-dir /d --fixed-now 2026-01-15T03:00:00Z --faults /f.json \
             --network-responses /n.json --control-socket /s.sock --route /ko/settings --window-size 1599x999",
        ))
        .unwrap();

        let qa = options.qa.expect("qa options");
        assert_eq!(qa.fixed_now_ms, 1_768_446_000_000);
        assert_eq!(qa.control_socket_path, Some(PathBuf::from("/s.sock")));
        assert_eq!(options.initial_route, "/ko/settings");
        assert_eq!(options.window_size, (1599.0, 999.0));
    }

    #[test]
    fn determinism_flags_are_rejected_outside_qa_mode() {
        assert_eq!(
            parse_launch_options(arguments("--faults /f.json")),
            Err(LaunchOptionsError::QaOnly("--faults".into()))
        );
        assert_eq!(
            parse_launch_options(arguments("--qa")),
            Err(LaunchOptionsError::MissingFixedNow)
        );
        let defaults = parse_launch_options(Vec::new()).unwrap();
        assert_eq!(defaults.initial_route, "/ko");
        assert_eq!(defaults.window_size, DEFAULT_WINDOW_SIZE);
    }
}
