//! TS 앱의 `HashRouter` 경로(`#/<locale>/...`)를 옮긴 라우팅 모델.
//! 경로 문자열은 알림 payload, 창 재사용 판정, QA 드라이버가 그대로 주고받으므로 TS와 같은 모양을 유지한다.

use kanvibe_domain::i18n::Locale;

/// 화면 하나에 해당하는 경로
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AppRoute {
    Board,
    TaskDetail {
        task_id: String,
    },
    TaskDiff {
        task_id: String,
    },
    Settings,
    ShortcutSettings,
    AiAccounts,
    PaneLayout,
    /// 어느 화면에도 맞지 않는 경로. locale 뒤 원래 조각을 그대로 들고 있어 히스토리가 사용자가 연 경로를 유지한다
    NotFound {
        unmatched_path: String,
    },
}

/// locale과 화면, 화면 로컬 쿼리(`?projects=` 등)를 함께 담은 현재 위치
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalizedRoute {
    pub locale: Locale,
    pub route: AppRoute,
    /// `?` 뒤 원문. 없으면 빈 문자열
    pub search: String,
}

/// 경로를 그릴지, 다른 경로로 바로 바꿀지
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResolution {
    Render(LocalizedRoute),
    /// TS `<Navigate replace>`처럼 히스토리를 늘리지 않고 이 경로로 바꾼다
    Redirect(String),
}

impl LocalizedRoute {
    pub fn new(locale: Locale, route: AppRoute) -> Self {
        Self {
            locale,
            route,
            search: String::new(),
        }
    }

    /// 히스토리와 창 재사용 판정에 쓰는 `/<locale>/...` 경로
    pub fn to_path(&self) -> String {
        let locale = self.locale.as_str();
        let path = match &self.route {
            AppRoute::Board => format!("/{locale}"),
            AppRoute::TaskDetail { task_id } => format!("/{locale}/task/{task_id}"),
            AppRoute::TaskDiff { task_id } => format!("/{locale}/task/{task_id}/diff"),
            AppRoute::Settings => format!("/{locale}/settings"),
            AppRoute::ShortcutSettings => format!("/{locale}/settings/shortcuts"),
            AppRoute::AiAccounts => format!("/{locale}/ai-accounts"),
            AppRoute::PaneLayout => format!("/{locale}/pane-layout"),
            AppRoute::NotFound { unmatched_path } => format!("/{locale}/{unmatched_path}"),
        };
        if self.search.is_empty() {
            path
        } else {
            format!("{path}?{}", self.search)
        }
    }
}

/// react-router 기본값처럼 정적 경로 조각은 대소문자를 가리지 않는다
fn is_segment(segment: &str, expected: &str) -> bool {
    segment.eq_ignore_ascii_case(expected)
}

fn match_route(rest: &[&str]) -> AppRoute {
    match rest {
        [] => AppRoute::Board,
        [settings] if is_segment(settings, "settings") => AppRoute::Settings,
        [settings, shortcuts]
            if is_segment(settings, "settings") && is_segment(shortcuts, "shortcuts") =>
        {
            AppRoute::ShortcutSettings
        }
        [accounts] if is_segment(accounts, "ai-accounts") => AppRoute::AiAccounts,
        [layout] if is_segment(layout, "pane-layout") => AppRoute::PaneLayout,
        [task, task_id] if is_segment(task, "task") => AppRoute::TaskDetail {
            task_id: (*task_id).to_string(),
        },
        [task, task_id, diff] if is_segment(task, "task") && is_segment(diff, "diff") => {
            AppRoute::TaskDiff {
                task_id: (*task_id).to_string(),
            }
        }
        _ => AppRoute::NotFound {
            unmatched_path: rest.join("/"),
        },
    }
}

/// `#` 뒤 경로를 읽는다. 루트와 모르는 locale은 기본 locale 보드로 보낸다.
pub fn resolve_route_path(hash_path: &str) -> RouteResolution {
    let (path, search) = hash_path.split_once('?').unwrap_or((hash_path, ""));
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let default_board = format!("/{}", Locale::DEFAULT.as_str());
    let Some((locale_segment, rest)) = segments.split_first() else {
        return RouteResolution::Redirect(default_board);
    };
    let Some(locale) = Locale::parse(locale_segment) else {
        return RouteResolution::Redirect(default_board);
    };
    RouteResolution::Render(LocalizedRoute {
        locale,
        route: match_route(rest),
        search: search.to_string(),
    })
}

/// 앱 안 링크에 현재 locale을 붙인다. 이미 locale로 시작하거나 `/`로 시작하지 않으면 그대로 둔다.
pub fn localize_href(href: &str, current_locale: Option<&str>) -> String {
    if !href.starts_with('/') {
        return href.to_string();
    }
    let locale = Locale::parse_or_default(current_locale);
    let first_segment = href.split('/').find(|segment| !segment.is_empty());
    if first_segment.is_some_and(|segment| Locale::parse(segment).is_some()) {
        return href.to_string();
    }
    if href == "/" {
        return format!("/{}", locale.as_str());
    }
    format!("/{}{href}", locale.as_str())
}

/// 작업 상세 경로. 알림 클릭과 창 재사용 판정이 같은 문자열을 쓴다.
pub fn task_detail_href(task_id: &str, current_locale: Option<&str>) -> String {
    localize_href(&format!("/task/{task_id}"), current_locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(path: &str) -> LocalizedRoute {
        match resolve_route_path(path) {
            RouteResolution::Render(route) => route,
            RouteResolution::Redirect(target) => panic!("{path} redirected to {target}"),
        }
    }

    #[test]
    fn index_path_renders_the_board() {
        assert_eq!(rendered("/ko").route, AppRoute::Board);
        assert_eq!(
            resolve_route_path("/"),
            RouteResolution::Redirect("/ko".into())
        );
    }

    #[test]
    fn settings_path_renders_the_settings_route() {
        assert_eq!(rendered("/ko/settings").route, AppRoute::Settings);
        assert_eq!(
            rendered("/en/settings/shortcuts").route,
            AppRoute::ShortcutSettings
        );
    }

    #[test]
    fn every_route_round_trips_through_its_path() {
        let routes = [
            AppRoute::Board,
            AppRoute::TaskDetail {
                task_id: "task-1".into(),
            },
            AppRoute::TaskDiff {
                task_id: "task-1".into(),
            },
            AppRoute::Settings,
            AppRoute::ShortcutSettings,
            AppRoute::AiAccounts,
            AppRoute::PaneLayout,
        ];
        for route in routes {
            let localized = LocalizedRoute::new(Locale::Zh, route);
            assert_eq!(rendered(&localized.to_path()), localized);
        }
    }

    #[test]
    fn unknown_locale_redirects_and_unknown_path_is_not_found() {
        assert_eq!(
            resolve_route_path("/fr/settings"),
            RouteResolution::Redirect("/ko".into())
        );
        let not_found = rendered("/ko/does-not-exist");
        assert_eq!(
            not_found.route,
            AppRoute::NotFound {
                unmatched_path: "does-not-exist".into()
            }
        );
        assert_eq!(not_found.to_path(), "/ko/does-not-exist");
        assert!(matches!(
            rendered("/ko/task/t1/diff/extra").route,
            AppRoute::NotFound { .. }
        ));
    }

    #[test]
    fn search_is_kept_beside_the_route() {
        let route = rendered("/ko?projects=p1");
        assert_eq!(route.route, AppRoute::Board);
        assert_eq!(route.to_path(), "/ko?projects=p1");
    }

    #[test]
    fn localize_href_adds_the_current_locale_only_to_unlocalized_app_paths() {
        assert_eq!(localize_href("/task/t1", Some("en")), "/en/task/t1");
        assert_eq!(localize_href("/", Some("zh")), "/zh");
        assert_eq!(localize_href("/ko/settings", Some("en")), "/ko/settings");
        assert_eq!(
            localize_href("https://example.com", Some("en")),
            "https://example.com"
        );
        assert_eq!(localize_href("/settings", Some("fr")), "/ko/settings");
    }
}
