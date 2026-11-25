//! TS 테스트 ledger(`parity/ts-cases.jsonl`)의 각 행이 Rust 테스트로 옮겨져 통과하는지, obsolete로 표시됐는지 집계한다.

mod annotations;
mod ledger;
mod obsolete;
mod report;
mod test_results;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct CheckOptions {
    /// 비어 있으면 모든 영역을 본다
    areas: BTreeSet<String>,
    should_list_unmapped: bool,
}

impl CheckOptions {
    pub fn parse(arguments: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            areas: BTreeSet::new(),
            should_list_unmapped: false,
        };
        let mut remaining = arguments.iter();
        while let Some(&argument) = remaining.next() {
            match argument {
                "--area" => {
                    let area = remaining
                        .next()
                        .ok_or("--area 뒤에 영역 이름이 필요합니다")?;
                    options.areas.insert(area.to_string());
                }
                "--list-unmapped" => options.should_list_unmapped = true,
                unknown => return Err(format!("알 수 없는 옵션: {unknown}")),
            }
        }
        Ok(options)
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask는 workspace 루트 바로 아래에 있다")
        .to_path_buf()
}

/// 집계를 출력하고, 매핑된 테스트가 모두 통과하고 매핑 오류가 없으면 `true`를 돌려준다
pub fn run_check(options: &CheckOptions) -> Result<bool, String> {
    let root = workspace_root();
    let ledger_rows = ledger::read_ledger(&root.join("parity/ts-cases.jsonl"))?;
    let obsolete_entries = obsolete::read_obsolete_entries(&root.join("parity/obsolete"))?;
    let test_annotations = annotations::scan_annotations(&root.join("crates"))?;
    let outcomes = test_results::run_workspace_tests(&root)?;

    let summary = report::summarize(
        &ledger_rows,
        &obsolete_entries,
        &test_annotations,
        &outcomes,
    );
    report::print_summary(&summary, &options.areas);
    if options.should_list_unmapped {
        report::print_unmapped(&summary, &options.areas);
    }
    Ok(summary.is_passing(&options.areas))
}
