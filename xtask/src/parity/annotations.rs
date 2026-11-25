//! `// parity: <ID>[, <ID>...]` 주석을 찾아 바로 아래 테스트 함수와 잇는다.

use std::path::{Component, Path};

/// nextest가 테스트를 부르는 이름의 앞부분. 패키지와 테스트 바이너리, 파일이 놓인 모듈 경로다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestLocation {
    pub package: String,
    pub binary: String,
    pub module_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestAnnotation {
    pub ledger_ids: Vec<String>,
    pub location: TestLocation,
    pub function_name: String,
    /// 사람이 찾아갈 수 있는 `파일:줄`
    pub source: String,
}

const ANNOTATION_PREFIX: &str = "// parity:";

/// `crates/<패키지>/src/a/b.rs`는 라이브러리 테스트 `a::b::`, `crates/<패키지>/tests/x.rs`는 테스트 바이너리 `x`다
pub fn test_location(crate_relative_path: &Path, package: &str) -> Option<TestLocation> {
    let parts: Vec<&str> = crate_relative_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    let (binary, module_parts) = match parts.as_slice() {
        ["src", module_parts @ ..] => (package.replace('-', "_"), module_parts),
        ["tests", file_name] => (file_name.strip_suffix(".rs")?.to_string(), &[][..]),
        ["tests", binary_directory, module_parts @ ..] => {
            (binary_directory.to_string(), module_parts)
        }
        _ => return None,
    };

    let module_prefix: String = module_parts
        .iter()
        .map(|part| part.strip_suffix(".rs").unwrap_or(part))
        .filter(|part| !["lib", "main", "mod"].contains(part))
        .map(|part| format!("{part}::"))
        .collect();
    Some(TestLocation {
        package: package.to_string(),
        binary,
        module_prefix,
    })
}

fn parse_ledger_ids(annotation: &str) -> Vec<String> {
    annotation
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect()
}

/// 주석 아래 속성과 주석 줄을 건너뛰고 처음 나오는 `fn 이름(`의 이름
fn following_function_name<'a>(lines: impl Iterator<Item = &'a str>) -> Option<String> {
    for line in lines.map(str::trim) {
        if line.starts_with("#[") || line.starts_with("//") || line.is_empty() {
            continue;
        }
        let after_fn = line.split_once("fn ")?.1;
        let name_length =
            after_fn.find(|character: char| !(character.is_alphanumeric() || character == '_'))?;
        return Some(after_fn[..name_length].to_string());
    }
    None
}

pub fn annotations_in_source(
    source: &str,
    location: &TestLocation,
    file_label: &str,
) -> Result<Vec<TestAnnotation>, String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut annotations = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let Some(annotation) = line.trim().strip_prefix(ANNOTATION_PREFIX) else {
            continue;
        };
        let source_label = format!("{file_label}:{}", line_index + 1);
        let function_name = following_function_name(lines[line_index + 1..].iter().copied())
            .ok_or_else(|| format!("{source_label}: parity 주석 아래에 테스트 함수가 없습니다"))?;
        annotations.push(TestAnnotation {
            ledger_ids: parse_ledger_ids(annotation),
            location: location.clone(),
            function_name,
            source: source_label,
        });
    }
    Ok(annotations)
}

fn collect_rust_files(directory: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() && path.file_name().is_some_and(|name| name != "target") {
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// `crates/` 아래 모든 패키지의 parity 주석
pub fn scan_annotations(crates_directory: &Path) -> Result<Vec<TestAnnotation>, String> {
    let mut files = Vec::new();
    collect_rust_files(crates_directory, &mut files)?;
    files.sort();

    let mut annotations = Vec::new();
    for file_path in files {
        let crate_relative = file_path
            .strip_prefix(crates_directory)
            .map_err(|error| error.to_string())?;
        let mut parts = crate_relative.components();
        let package = parts
            .next()
            .and_then(|part| part.as_os_str().to_str())
            .unwrap_or_default()
            .to_string();
        let Some(location) = test_location(parts.as_path(), &package) else {
            continue;
        };
        let source = std::fs::read_to_string(&file_path)
            .map_err(|error| format!("{}: {error}", file_path.display()))?;
        annotations.extend(annotations_in_source(
            &source,
            &location,
            &file_path.display().to_string(),
        )?);
    }
    Ok(annotations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_follow_the_module_tree() {
        let location = |path: &str| test_location(Path::new(path), "kanvibe-db").unwrap();

        assert_eq!(location("src/lib.rs").module_prefix, "");
        assert_eq!(
            location("src/migrations/mod.rs").module_prefix,
            "migrations::"
        );
        assert_eq!(
            location("src/migrations/tests.rs").module_prefix,
            "migrations::tests::"
        );
        assert_eq!(location("src/migrations/tests.rs").binary, "kanvibe_db");
        assert_eq!(
            location("tests/ts_reference_compat.rs").binary,
            "ts_reference_compat"
        );
        assert_eq!(test_location(Path::new("Cargo.toml"), "kanvibe-db"), None);
    }

    #[test]
    fn annotation_maps_to_the_next_function_after_attributes() {
        let location = test_location(Path::new("src/task.rs"), "kanvibe-domain").unwrap();
        let source = "// parity: TS-a-1, TS-a-2\n#[test]\nfn first_case() {}\n\n    // parity: TS-a-3\n    #[test]\n    fn second_case() {}\n";

        let annotations = annotations_in_source(source, &location, "task.rs").unwrap();

        assert_eq!(annotations[0].ledger_ids, ["TS-a-1", "TS-a-2"]);
        assert_eq!(annotations[0].function_name, "first_case");
        assert_eq!(annotations[1].function_name, "second_case");
        assert!(
            annotations_in_source("// parity: TS-a-1\nconst X: u8 = 1;\n", &location, "x.rs")
                .is_err()
        );
    }
}
