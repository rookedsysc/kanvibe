//! 변경 파일 트리(TS `DiffFileTree`). 경로를 폴더 트리로 묶고, 폴더를 먼저·이름순(`localeCompare`)으로 놓는다.
//! 폴더는 처음에 모두 펼쳐 두고, 파일 줄에는 변경량 점과 상태 약어(A·M·D·R)를 붙인다. 확인한 파일은 체크 아이콘과 취소선으로 흐리게 그린다.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use gpui_kit::{AnyElement, IntoElement, ParentElement, Rgba, Styled, div, px};
use kanvibe_domain::locale_order::compare_like_locale;
use kanvibe_domain::task_diff_stats::{DiffFile, DiffFileStatus};

use super::icons::{DiffIcon, diff_icon};
use super::text_baseline::{TextFamily, aligned_text, chromium_text};
use crate::components::fade;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::TailwindText;

/// 줄 들여쓰기: `depth * 14 + 8`
const DEPTH_INDENT: f32 = 14.0;
const BASE_INDENT: f32 = 8.0;
const MAX_CHANGE_DOTS: i64 = 5;

/// Tailwind 팔레트 색(`text-green-600` 등). 의미 토큰이 없는 diff 전용 색이라 이 화면에 둔다
pub mod change_colors {
    use gpui_kit::{Rgba, rgb};

    pub fn green_500() -> Rgba {
        rgb(0x00C951)
    }
    pub fn green_600() -> Rgba {
        rgb(0x00A63E)
    }
    pub fn red_500() -> Rgba {
        rgb(0xFB2C36)
    }
    pub fn yellow_600() -> Rgba {
        rgb(0xD08700)
    }
    pub fn blue_500() -> Rgba {
        rgb(0x2B7FFF)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTreeRowKind {
    Folder {
        is_expanded: bool,
    },
    File {
        status: DiffFileStatus,
        additions: i64,
        deletions: i64,
    },
}

/// 화면에 보이는 트리 한 줄
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeRow {
    pub full_path: String,
    pub name: String,
    pub depth: usize,
    pub kind: FileTreeRowKind,
}

#[derive(Default)]
struct TreeNode {
    full_path: String,
    file: Option<DiffFile>,
    children: BTreeMap<String, TreeNode>,
}

fn build_tree(files: &[DiffFile]) -> TreeNode {
    let mut root = TreeNode::default();
    for file in files {
        let segments: Vec<&str> = file.path.split('/').collect();
        let mut current = &mut root;
        for (index, segment) in segments.iter().enumerate() {
            let is_last = index + 1 == segments.len();
            current = current
                .children
                .entry(segment.to_string())
                .or_insert_with(|| TreeNode {
                    full_path: segments[..=index].join("/"),
                    file: is_last.then(|| file.clone()),
                    children: BTreeMap::new(),
                });
        }
    }
    root
}

/// 폴더 먼저, 같은 종류끼리는 `localeCompare` 순서
fn sorted_children(node: &TreeNode) -> Vec<(&String, &TreeNode)> {
    let mut children: Vec<_> = node.children.iter().collect();
    children.sort_by(|(left_name, left), (right_name, right)| {
        match (left.file.is_some(), right.file.is_some()) {
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            _ => compare_like_locale(left_name, right_name),
        }
    });
    children
}

/// 처음 그릴 때 펼쳐 둘 폴더(모든 폴더)
pub fn all_folder_paths(files: &[DiffFile]) -> HashSet<String> {
    let mut folders = HashSet::new();
    for file in files {
        let segments: Vec<&str> = file.path.split('/').collect();
        for depth in 1..segments.len() {
            folders.insert(segments[..depth].join("/"));
        }
    }
    folders
}

/// 펼친 폴더 안쪽까지 화면에 보일 줄을 순서대로 모은다
pub fn visible_tree_rows(
    files: &[DiffFile],
    expanded_folders: &HashSet<String>,
) -> Vec<FileTreeRow> {
    let tree = build_tree(files);
    let mut rows = Vec::new();
    collect_rows(&tree, 0, expanded_folders, &mut rows);
    rows
}

fn collect_rows(
    node: &TreeNode,
    depth: usize,
    expanded_folders: &HashSet<String>,
    rows: &mut Vec<FileTreeRow>,
) {
    for (name, child) in sorted_children(node) {
        let kind = match &child.file {
            Some(file) => FileTreeRowKind::File {
                status: file.status,
                additions: file.additions,
                deletions: file.deletions,
            },
            None => FileTreeRowKind::Folder {
                is_expanded: expanded_folders.contains(&child.full_path),
            },
        };
        let is_expanded_folder = kind == FileTreeRowKind::Folder { is_expanded: true };
        rows.push(FileTreeRow {
            full_path: child.full_path.clone(),
            name: name.clone(),
            depth,
            kind,
        });
        if is_expanded_folder {
            collect_rows(child, depth + 1, expanded_folders, rows);
        }
    }
}

pub fn status_label(status: DiffFileStatus) -> &'static str {
    match status {
        DiffFileStatus::Added => "A",
        DiffFileStatus::Modified => "M",
        DiffFileStatus::Deleted => "D",
        DiffFileStatus::Renamed => "R",
    }
}

fn status_color(status: DiffFileStatus) -> Rgba {
    match status {
        DiffFileStatus::Added => change_colors::green_600(),
        DiffFileStatus::Modified => change_colors::yellow_600(),
        DiffFileStatus::Deleted => change_colors::red_500(),
        DiffFileStatus::Renamed => change_colors::blue_500(),
    }
}

/// 추가·삭제 비율을 점 다섯 개로 나눈다(`Math.round(additions / total * 5)`)
pub fn change_dot_split(additions: i64, deletions: i64) -> Option<(usize, usize)> {
    let total = additions + deletions;
    if total == 0 {
        return None;
    }
    let addition_dots =
        ((additions as f64 / total as f64) * MAX_CHANGE_DOTS as f64).round() as usize;
    Some((addition_dots, MAX_CHANGE_DOTS as usize - addition_dots))
}

/// 트리 줄 하나의 상태
pub struct FileRowState {
    pub is_selected: bool,
    pub is_viewed: bool,
}

/// `w-full flex items-center gap-1.5 px-2 py-1 text-xs rounded` 줄 틀
pub fn row_frame(row: &FileTreeRow) -> gpui_kit::Div {
    div()
        .relative()
        .w_full()
        .flex()
        .items_center()
        .gap(spacing(1.5))
        .pr(spacing(2.0))
        .pl(px(row.depth as f32 * DEPTH_INDENT + BASE_INDENT))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .rounded(radius::BASE)
}

/// 폴더 줄 내용: 펼침 화살표, 폴더 아이콘, `font-medium` 이름
pub fn folder_row_children(
    theme: &ThemeTokens,
    row: &FileTreeRow,
    is_expanded: bool,
) -> Vec<AnyElement> {
    vec![
        diff_icon(DiffIcon::FolderChevron { is_expanded }, theme.text.primary).into_any_element(),
        diff_icon(
            DiffIcon::Folder {
                is_open: is_expanded,
            },
            change_colors::blue_500(),
        )
        .into_any_element(),
        div()
            .truncate()
            .css_weight(500)
            .text_color(theme.text.primary)
            .child(aligned_text(
                row.name.clone(),
                text_scale::XS,
                TextFamily::Sans,
            ))
            .into_any_element(),
    ]
}

/// 파일 줄의 글자색. 선택 > 확인 > 기본
pub fn file_row_text_color(theme: &ThemeTokens, state: &FileRowState) -> Rgba {
    if state.is_selected {
        theme.text.brand
    } else if state.is_viewed {
        theme.text.muted
    } else {
        theme.text.secondary
    }
}

/// 확인한 파일은 파일 아이콘 대신 체크를 보인다
pub fn file_row_icon(state: &FileRowState) -> DiffIcon {
    if state.is_viewed {
        DiffIcon::Viewed
    } else {
        DiffIcon::File
    }
}

/// 확인한 파일 이름은 취소선과 `opacity-60`으로 흐린다
pub fn file_name_is_struck_through(state: &FileRowState) -> bool {
    state.is_viewed
}

/// 파일 줄 내용: 아이콘, 이름, 변경량 점, 상태 약어
pub fn file_row_children(
    theme: &ThemeTokens,
    row: &FileTreeRow,
    state: &FileRowState,
) -> Vec<AnyElement> {
    let FileTreeRowKind::File {
        status,
        additions,
        deletions,
    } = row.kind
    else {
        return Vec::new();
    };
    let icon_color = if state.is_viewed {
        change_colors::green_500()
    } else {
        theme.text.muted
    };
    let icon = diff_icon(file_row_icon(state), icon_color);
    let name = file_name(row, state);
    let mut children = vec![icon.into_any_element(), name.into_any_element()];
    if !state.is_viewed
        && let Some(dots) = change_dots(additions, deletions)
    {
        children.push(dots.into_any_element());
    }
    let label = status_badge(status, state);
    children.push(label.into_any_element());
    children
}

/// `truncate flex-1` 파일 이름. 선택하면 `font-medium`, 확인하면 취소선과 `opacity-60`
fn file_name(row: &FileTreeRow, state: &FileRowState) -> gpui_kit::Div {
    let name = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .css_weight(if state.is_selected { 500 } else { 400 })
        .child(aligned_text(
            row.name.clone(),
            text_scale::XS,
            TextFamily::Sans,
        ));
    if file_name_is_struck_through(state) {
        name.line_through().opacity(0.6)
    } else {
        name
    }
}

/// `font-mono text-[10px] font-bold` 상태 약어. 확인한 파일은 `opacity-40`
fn status_badge(status: DiffFileStatus, state: &FileRowState) -> gpui_kit::Div {
    let color = if state.is_viewed {
        fade(status_color(status), 0.4)
    } else {
        status_color(status)
    };
    div()
        .flex_none()
        .mono(700)
        .text_size(px(10.0))
        .line_height(px(40.0 / 3.0))
        .text_color(color)
        .child(chromium_text(status_label(status)))
}

/// `flex gap-px ml-1.5`의 6px 원 점
fn change_dots(additions: i64, deletions: i64) -> Option<impl IntoElement> {
    let (addition_dots, deletion_dots) = change_dot_split(additions, deletions)?;
    let dot = |color: Rgba| div().size(px(6.0)).rounded(radius::FULL).bg(color);
    let dots = (0..addition_dots)
        .map(|_| dot(change_colors::green_500()))
        .chain((0..deletion_dots).map(|_| dot(change_colors::red_500())));
    Some(
        div()
            .flex()
            .flex_none()
            .gap(px(1.0))
            .ml(spacing(1.5))
            .children(dots),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, status: DiffFileStatus) -> DiffFile {
        DiffFile {
            path: path.into(),
            original_path: None,
            status,
            additions: 5,
            deletions: 2,
        }
    }

    fn names(rows: &[FileTreeRow]) -> Vec<(usize, String)> {
        rows.iter()
            .map(|row| (row.depth, row.name.clone()))
            .collect()
    }

    // parity: TS-components-DiffFileTree-c2fcc0
    #[test]
    fn flat_paths_become_a_nested_tree_with_folders_first() {
        let files = vec![
            file("src/components/Button.tsx", DiffFileStatus::Added),
            file("src/components/Modal.tsx", DiffFileStatus::Modified),
            file("README.md", DiffFileStatus::Modified),
        ];

        let rows = visible_tree_rows(&files, &all_folder_paths(&files));

        assert_eq!(
            names(&rows),
            vec![
                (0, "src".into()),
                (1, "components".into()),
                (2, "Button.tsx".into()),
                (2, "Modal.tsx".into()),
                (0, "README.md".into()),
            ]
        );
    }

    // parity: TS-components-DiffFileTree-801670
    #[test]
    fn every_status_has_its_one_letter_label() {
        let labels: Vec<_> = [
            DiffFileStatus::Added,
            DiffFileStatus::Modified,
            DiffFileStatus::Deleted,
            DiffFileStatus::Renamed,
        ]
        .into_iter()
        .map(status_label)
        .collect();

        assert_eq!(labels, vec!["A", "M", "D", "R"]);
    }

    #[test]
    fn collapsed_folders_hide_their_descendants() {
        let files = vec![file("src/deep/file.ts", DiffFileStatus::Modified)];
        let mut expanded = all_folder_paths(&files);
        expanded.remove("src");

        let rows = visible_tree_rows(&files, &expanded);

        assert_eq!(names(&rows), vec![(0, "src".into())]);
    }

    #[test]
    fn change_dots_split_five_by_the_addition_ratio() {
        assert_eq!(change_dot_split(5, 1), Some((4, 1)));
        assert_eq!(change_dot_split(4, 0), Some((5, 0)));
        assert_eq!(change_dot_split(0, 0), None);
    }
}
