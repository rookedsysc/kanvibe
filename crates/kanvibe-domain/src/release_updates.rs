//! 릴리스 태그 비교. 사전 릴리스 태그(`1.2.3-beta.1`)는 업데이트 대상으로 보지 않는다.

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// JS `Number.MAX_SAFE_INTEGER`. 이보다 큰 번호는 TS 앱이 거부했으므로 같은 태그를 같은 결과로 판정하려고 맞춘다.
const MAX_SAFE_VERSION_PART: u64 = (1 << 53) - 1;

/// `1.2.3` 또는 `v1.2.3`만 받아들인다
pub fn parse_release_version(value: &str) -> Option<ReleaseVersion> {
    let trimmed_value = value.trim();
    let version_text = trimmed_value.strip_prefix('v').unwrap_or(trimmed_value);
    let parts: Vec<&str> = version_text.split('.').collect();
    let [major, minor, patch] = parts.as_slice() else {
        return None;
    };

    Some(ReleaseVersion {
        major: parse_version_part(major)?,
        minor: parse_version_part(minor)?,
        patch: parse_version_part(patch)?,
    })
}

fn parse_version_part(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse::<u64>()
        .ok()
        .filter(|&number| number <= MAX_SAFE_VERSION_PART)
}

pub fn compare_release_versions(left: ReleaseVersion, right: ReleaseVersion) -> Ordering {
    left.cmp(&right)
}

pub fn is_release_version_greater_than(candidate: &str, current: &str) -> bool {
    match (
        parse_release_version(candidate),
        parse_release_version(current),
    ) {
        (Some(candidate_version), Some(current_version)) => {
            compare_release_versions(candidate_version, current_version) == Ordering::Greater
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u64, minor: u64, patch: u64) -> ReleaseVersion {
        ReleaseVersion {
            major,
            minor,
            patch,
        }
    }

    // parity: TS-shared-releaseUpdates-131111
    #[test]
    fn parses_plain_and_v_prefixed_versions() {
        assert_eq!(parse_release_version("1.2.3"), Some(version(1, 2, 3)));
        assert_eq!(
            parse_release_version("v10.20.30"),
            Some(version(10, 20, 30))
        );
    }

    // parity: TS-shared-releaseUpdates-6a1bd5
    #[test]
    fn rejects_invalid_or_pre_release_tags() {
        assert_eq!(parse_release_version("1.2"), None);
        assert_eq!(parse_release_version("1.2.3-beta.1"), None);
        assert_eq!(parse_release_version("release-1.2.3"), None);
        assert_eq!(parse_release_version("9007199254740992.0.0"), None);
    }

    // parity: TS-shared-releaseUpdates-c40f72
    #[test]
    fn compares_by_major_minor_and_patch() {
        assert_eq!(
            compare_release_versions(version(2, 0, 0), version(1, 9, 9)),
            Ordering::Greater
        );
        assert_eq!(
            compare_release_versions(version(1, 3, 0), version(1, 2, 9)),
            Ordering::Greater
        );
        assert_eq!(
            compare_release_versions(version(1, 2, 4), version(1, 2, 3)),
            Ordering::Greater
        );
        assert_eq!(
            compare_release_versions(version(1, 2, 3), version(1, 2, 3)),
            Ordering::Equal
        );
    }

    // parity: TS-shared-releaseUpdates-5d9382
    #[test]
    fn only_greater_semver_releases_count() {
        assert!(is_release_version_greater_than("1.0.1", "1.0.0"));
        assert!(is_release_version_greater_than("v1.1.0", "1.0.9"));
        assert!(!is_release_version_greater_than("1.0.0", "1.0.0"));
        assert!(!is_release_version_greater_than("0.9.9", "1.0.0"));
        assert!(!is_release_version_greater_than("1.1.0-beta.1", "1.0.0"));
    }
}
