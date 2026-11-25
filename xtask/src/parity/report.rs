//! ledger 행마다 매핑·obsolete 상태를 정하고 영역별로 센다.

use std::collections::{BTreeMap, BTreeSet};

use super::annotations::TestAnnotation;
use super::ledger::LedgerRow;
use super::obsolete::ObsoleteEntries;
use super::test_results::{TestName, TestOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStatus {
    MappedPassing,
    MappedFailing,
    Obsolete,
    Unmapped,
}

#[derive(Debug, Default)]
pub struct AreaCounts {
    pub total: usize,
    pub mapped_passing: usize,
    pub mapped_failing: usize,
    pub obsolete: usize,
    pub unmapped: usize,
}

pub struct ParitySummary {
    pub row_statuses: Vec<(LedgerRow, RowStatus)>,
    /// ledger에 없는 ID, 테스트를 찾지 못한 주석, 매핑과 obsolete가 겹친 ID 같은 매핑 자체의 오류
    pub mapping_errors: Vec<String>,
}

/// 주석이 가리키는 nextest 테스트. 파일 모듈 경로 아래에서 함수 이름으로 끝나는 테스트가 정확히 하나여야 한다
fn resolve_annotation(
    annotation: &TestAnnotation,
    outcomes: &BTreeMap<TestName, TestOutcome>,
) -> Result<TestOutcome, String> {
    let function_suffix = format!("::{}", annotation.function_name);
    let candidates: Vec<TestOutcome> = outcomes
        .iter()
        .filter(|(name, _)| {
            let location = &annotation.location;
            let full_path = format!("::{}", name.test_path);
            name.package == location.package
                && name.binary == location.binary
                && name.test_path.starts_with(&location.module_prefix)
                && full_path.ends_with(&function_suffix)
        })
        .map(|(_, outcome)| *outcome)
        .collect();
    match candidates.as_slice() {
        [outcome] => Ok(*outcome),
        [] => Err(format!(
            "{}: `{}` 테스트 결과가 없습니다",
            annotation.source, annotation.function_name
        )),
        _ => Err(format!(
            "{}: `{}`에 해당하는 테스트가 여러 개입니다",
            annotation.source, annotation.function_name
        )),
    }
}

/// 행 ID별로 매핑된 테스트가 모두 통과했는지
fn mapped_results(
    annotations: &[TestAnnotation],
    outcomes: &BTreeMap<TestName, TestOutcome>,
    mapping_errors: &mut Vec<String>,
) -> BTreeMap<String, bool> {
    let mut is_passing_by_id: BTreeMap<String, bool> = BTreeMap::new();
    for annotation in annotations {
        let is_passing = match resolve_annotation(annotation, outcomes) {
            Ok(outcome) => outcome == TestOutcome::Passed,
            Err(message) => {
                mapping_errors.push(message);
                false
            }
        };
        for ledger_id in &annotation.ledger_ids {
            let entry = is_passing_by_id.entry(ledger_id.clone()).or_insert(true);
            *entry &= is_passing;
        }
    }
    is_passing_by_id
}

pub fn summarize(
    ledger_rows: &[LedgerRow],
    obsolete_entries: &ObsoleteEntries,
    annotations: &[TestAnnotation],
    outcomes: &BTreeMap<TestName, TestOutcome>,
) -> ParitySummary {
    let mut mapping_errors = Vec::new();
    let is_passing_by_id = mapped_results(annotations, outcomes, &mut mapping_errors);
    let ledger_ids: BTreeSet<&str> = ledger_rows.iter().map(|row| row.id.as_str()).collect();
    let referenced_ids = is_passing_by_id.keys().chain(obsolete_entries.keys());
    for unknown_id in referenced_ids.filter(|id| !ledger_ids.contains(id.as_str())) {
        mapping_errors.push(format!("ledger에 없는 ID: {unknown_id}"));
    }

    let row_statuses = ledger_rows
        .iter()
        .map(|row| {
            let is_obsolete = obsolete_entries.contains_key(&row.id);
            let status = match is_passing_by_id.get(&row.id) {
                Some(_) if is_obsolete => {
                    mapping_errors.push(format!("{}: 매핑과 obsolete가 겹칩니다", row.id));
                    RowStatus::MappedFailing
                }
                Some(true) => RowStatus::MappedPassing,
                Some(false) => RowStatus::MappedFailing,
                None if is_obsolete => RowStatus::Obsolete,
                None => RowStatus::Unmapped,
            };
            (row.clone(), status)
        })
        .collect();
    ParitySummary {
        row_statuses,
        mapping_errors,
    }
}

fn is_selected(row: &LedgerRow, areas: &BTreeSet<String>) -> bool {
    areas.is_empty() || areas.contains(row.area())
}

impl ParitySummary {
    fn counts_by_area(&self, areas: &BTreeSet<String>) -> BTreeMap<String, AreaCounts> {
        let mut counts: BTreeMap<String, AreaCounts> = BTreeMap::new();
        for (row, status) in self
            .row_statuses
            .iter()
            .filter(|(row, _)| is_selected(row, areas))
        {
            let area_counts = counts.entry(row.area().to_string()).or_default();
            area_counts.total += 1;
            match status {
                RowStatus::MappedPassing => area_counts.mapped_passing += 1,
                RowStatus::MappedFailing => area_counts.mapped_failing += 1,
                RowStatus::Obsolete => area_counts.obsolete += 1,
                RowStatus::Unmapped => area_counts.unmapped += 1,
            }
        }
        counts
    }

    pub fn is_passing(&self, areas: &BTreeSet<String>) -> bool {
        let has_failing_row = self
            .row_statuses
            .iter()
            .any(|(row, status)| is_selected(row, areas) && *status == RowStatus::MappedFailing);
        self.mapping_errors.is_empty() && !has_failing_row
    }
}

pub fn print_summary(summary: &ParitySummary, areas: &BTreeSet<String>) {
    println!(
        "{:<12} {:>6} {:>8} {:>8} {:>9} {:>9}",
        "area", "total", "passing", "failing", "obsolete", "unmapped"
    );
    let mut totals = AreaCounts::default();
    for (area, counts) in summary.counts_by_area(areas) {
        println!(
            "{area:<12} {:>6} {:>8} {:>8} {:>9} {:>9}",
            counts.total,
            counts.mapped_passing,
            counts.mapped_failing,
            counts.obsolete,
            counts.unmapped
        );
        totals.total += counts.total;
        totals.mapped_passing += counts.mapped_passing;
        totals.mapped_failing += counts.mapped_failing;
        totals.obsolete += counts.obsolete;
        totals.unmapped += counts.unmapped;
    }
    println!(
        "{:<12} {:>6} {:>8} {:>8} {:>9} {:>9}",
        "TOTAL",
        totals.total,
        totals.mapped_passing,
        totals.mapped_failing,
        totals.obsolete,
        totals.unmapped
    );
    for (row, _) in summary
        .row_statuses
        .iter()
        .filter(|(row, status)| is_selected(row, areas) && *status == RowStatus::MappedFailing)
    {
        println!("FAILING {} {} :: {}", row.id, row.file, row.title);
    }
    for message in &summary.mapping_errors {
        println!("MAPPING ERROR {message}");
    }
}

pub fn print_unmapped(summary: &ParitySummary, areas: &BTreeSet<String>) {
    let unmapped_rows = summary
        .row_statuses
        .iter()
        .filter(|(row, status)| is_selected(row, areas) && *status == RowStatus::Unmapped);
    for (row, _) in unmapped_rows {
        println!("UNMAPPED {} {} :: {}", row.id, row.file, row.title);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::parity::annotations::test_location;

    fn ledger_row(id: &str) -> LedgerRow {
        LedgerRow {
            id: id.into(),
            file: "src/a.test.ts".into(),
            title: "case".into(),
        }
    }

    fn annotation(ledger_id: &str, function_name: &str) -> TestAnnotation {
        TestAnnotation {
            ledger_ids: vec![ledger_id.into()],
            location: test_location(Path::new("src/task.rs"), "kanvibe-domain").unwrap(),
            function_name: function_name.into(),
            source: "task.rs:1".into(),
        }
    }

    fn outcome(test_path: &str, test_outcome: TestOutcome) -> (TestName, TestOutcome) {
        let name = TestName::parse(&format!("kanvibe-domain::kanvibe_domain${test_path}")).unwrap();
        (name, test_outcome)
    }

    #[test]
    fn classifies_rows_and_fails_on_a_failing_mapped_test() {
        let rows = [
            ledger_row("TS-shared-a-1"),
            ledger_row("TS-shared-a-2"),
            ledger_row("TS-lib-a-3"),
            ledger_row("TS-lib-a-4"),
        ];
        let obsolete_entries =
            ObsoleteEntries::from([("TS-lib-a-3".to_string(), "이유".to_string())]);
        let annotations = [
            annotation("TS-shared-a-1", "passes"),
            annotation("TS-shared-a-2", "fails"),
        ];
        let outcomes = BTreeMap::from([
            outcome("task::tests::passes", TestOutcome::Passed),
            outcome("task::tests::fails", TestOutcome::Failed),
        ]);

        let summary = summarize(&rows, &obsolete_entries, &annotations, &outcomes);
        let statuses: Vec<RowStatus> = summary
            .row_statuses
            .iter()
            .map(|(_, status)| *status)
            .collect();

        assert_eq!(
            statuses,
            [
                RowStatus::MappedPassing,
                RowStatus::MappedFailing,
                RowStatus::Obsolete,
                RowStatus::Unmapped
            ]
        );
        assert!(!summary.is_passing(&BTreeSet::new()));
        assert!(summary.is_passing(&BTreeSet::from(["lib".to_string()])));
    }

    #[test]
    fn unknown_ids_and_missing_tests_are_mapping_errors() {
        let rows = [ledger_row("TS-shared-a-1")];
        let annotations = [
            annotation("TS-shared-zz-9", "passes"),
            annotation("TS-shared-a-1", "missing"),
        ];
        let outcomes = BTreeMap::from([outcome("task::tests::passes", TestOutcome::Passed)]);

        let summary = summarize(&rows, &ObsoleteEntries::new(), &annotations, &outcomes);

        assert_eq!(
            summary.mapping_errors.len(),
            2,
            "{:?}",
            summary.mapping_errors
        );
        assert!(!summary.is_passing(&BTreeSet::new()));
    }
}
