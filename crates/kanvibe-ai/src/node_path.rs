//! Node `path`(POSIX) 함수를 문자열 그대로 옮긴다. 원격 경로도 같은 규칙으로 다루므로 `PathBuf`가 아닌 `&str`을 쓴다.

pub const SEPARATOR: char = '/';

/// Node `path.normalize`
pub fn normalize(value: &str) -> String {
    if value.is_empty() {
        return ".".to_string();
    }
    let is_absolute = value.starts_with('/');
    let has_trailing_separator = value.ends_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in value.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| *last != "..") {
                    segments.pop();
                } else if !is_absolute {
                    segments.push("..");
                }
            }
            other => segments.push(other),
        }
    }
    let mut normalized = segments.join("/");
    if normalized.is_empty() && !is_absolute {
        normalized.push('.');
    }
    if has_trailing_separator && !normalized.is_empty() && normalized != "." {
        normalized.push('/');
    } else if has_trailing_separator && normalized == "." {
        normalized = "./".to_string();
    }
    if is_absolute {
        format!("/{normalized}")
    } else {
        normalized
    }
}

/// Node `path.join`
pub fn join(parts: &[&str]) -> String {
    let joined = parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/");
    if joined.is_empty() {
        return ".".to_string();
    }
    normalize(&joined)
}

/// Node `path.resolve`. 상대 경로는 프로세스 현재 디렉터리를 앞에 붙이고, 끝의 `/`는 뗀다
pub fn resolve(parts: &[&str]) -> String {
    let mut resolved = String::new();
    for part in parts.iter().rev().filter(|part| !part.is_empty()) {
        resolved = if resolved.is_empty() {
            part.to_string()
        } else {
            format!("{part}/{resolved}")
        };
        if part.starts_with('/') {
            break;
        }
    }
    if !resolved.starts_with('/') {
        let current_directory = std::env::current_dir()
            .map(|directory| directory.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/".to_string());
        resolved = if resolved.is_empty() {
            current_directory
        } else {
            format!("{current_directory}/{resolved}")
        };
    }
    let normalized = normalize(&resolved);
    if normalized.len() > 1 {
        normalized.trim_end_matches('/').to_string()
    } else {
        normalized
    }
}

pub fn is_absolute(value: &str) -> bool {
    value.starts_with('/')
}

/// Node `path.dirname`
pub fn dirname(value: &str) -> String {
    if value.is_empty() {
        return ".".to_string();
    }
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    match trimmed.rfind('/') {
        None => ".".to_string(),
        Some(0) => "/".to_string(),
        Some(index) => {
            let parent = trimmed[..index].trim_end_matches('/');
            if parent.is_empty() {
                "/".to_string()
            } else {
                parent.to_string()
            }
        }
    }
}

/// Node `path.basename(value)`
pub fn basename(value: &str) -> String {
    let trimmed = value.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or_default().to_string()
}

/// Node `path.basename(value, extension)`. 이름 전체가 확장자와 같으면 떼지 않는다
pub fn basename_without(value: &str, extension: &str) -> String {
    let base = basename(value);
    match base.strip_suffix(extension) {
        Some(stem) if !stem.is_empty() => stem.to_string(),
        _ => base,
    }
}

/// Node `path.extname`
pub fn extname(value: &str) -> String {
    let base = basename(value);
    match base.rfind('.') {
        Some(index) if index > 0 && !base[..index].chars().all(|character| character == '.') => {
            base[index..].to_string()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_node_posix_path() {
        assert_eq!(join(&["/a/b", "../c", "d.jsonl"]), "/a/c/d.jsonl");
        assert_eq!(join(&["a", "", "b/"]), "a/b/");
        assert_eq!(resolve(&["/a/b/", "./c/"]), "/a/b/c");
        assert_eq!(resolve(&["/x", "/y", "z"]), "/y/z");
        assert_eq!(dirname("/a/b/c.json"), "/a/b");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(dirname("a"), ".");
        assert_eq!(basename("/a/b/c.json"), "c.json");
        assert_eq!(basename_without("/a/b/c.json", ".json"), "c");
        assert_eq!(extname("/a/b/c.JSONL"), ".JSONL");
        assert_eq!(extname("/a/.hidden"), "");
        assert_eq!(normalize("/../a/./b//c"), "/a/b/c");
    }
}
