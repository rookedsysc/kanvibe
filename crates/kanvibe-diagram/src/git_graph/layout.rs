//! gitGraphRenderer.ts의 자리 계산: 브랜치 줄 위치, 커밋 위치, 커밋 사이 화살표 경로.

use std::collections::HashMap;

use crate::scene::Point;
use crate::text::{FontBook, FontWeight};

use super::parse::{Commit, CommitKind, Direction, GitGraph};

/// 커밋 원 중심을 줄에서 띄우는 거리(LAYOUT_OFFSET)
pub const LAYOUT_OFFSET: f64 = 10.0;
const COMMIT_STEP: f64 = 40.0;
/// 세로 방향에서 첫 커밋 자리(defaultPos)
pub const VERTICAL_START: f64 = 30.0;
const BRANCH_SPACING: f64 = 50.0;
/// 커밋 라벨을 돌리면 브랜치 사이를 더 띄운다
const ROTATED_LABEL_SPACING: f64 = 40.0;
pub const BRANCH_FONT_SIZE: f64 = 16.0;
const NORMAL_CORNER_RADIUS: f64 = 20.0;
const REROUTE_CORNER_RADIUS: f64 = 10.0;
const LANE_GAP: f64 = 10.0;
const MAX_LANE_DEPTH: usize = 5;

#[derive(Clone, Copy, Debug)]
pub struct BranchPlace {
    /// LR이면 y, 세로면 x
    pub pos: f64,
    pub index: usize,
}

/// 커밋 원 중심과 라벨이 쓰는 진행 방향 자리(`pos`)
#[derive(Clone, Copy, Debug)]
pub struct CommitPlace {
    pub center: Point,
    pub pos: f64,
}

pub struct GraphLayout {
    pub direction: Direction,
    pub branches: HashMap<String, BranchPlace>,
    pub commits: HashMap<String, CommitPlace>,
    /// 마지막 커밋 다음 자리. 브랜치 줄이 여기까지 간다
    pub max_pos: f64,
    /// 화살표 우회 줄이 겹치지 않게 이미 쓴 자리(브랜치 줄 포함)
    lanes: Vec<f64>,
}

/// 화살표 하나: 꺾이는 점들과 모서리 반지름, 색 번호
pub struct ArrowPath {
    pub points: Vec<Point>,
    pub radius: f64,
    pub color_index: usize,
}

impl GraphLayout {
    pub fn new(graph: &GitGraph, fonts: &FontBook) -> Self {
        let mut layout = Self {
            direction: graph.direction,
            branches: HashMap::new(),
            commits: HashMap::new(),
            max_pos: 0.0,
            lanes: Vec::new(),
        };
        layout.place_branches(graph, fonts);
        layout.place_commits(graph);
        layout
    }

    /// setBranchPosition: 세로 방향은 브랜치 이름 폭의 절반을 더 띄운다
    fn place_branches(&mut self, graph: &GitGraph, fonts: &FontBook) {
        let spacing = if graph.options.rotate_commit_label {
            BRANCH_SPACING + ROTATED_LABEL_SPACING
        } else {
            BRANCH_SPACING
        };
        let mut pos = 0.0;
        for (index, name) in graph.branches.iter().enumerate() {
            self.branches
                .insert(name.clone(), BranchPlace { pos, index });
            pos += spacing;
            if self.direction.is_vertical() {
                pos += branch_label_width(fonts, name) / 2.0;
            }
        }
    }

    /// drawCommits: 순번 순서(BT는 거꾸로)로 50씩 나아간다. `parallelCommits`이면 가장 앞선 부모에서 40 뒤에
    /// 둬서 서로 다른 브랜치의 커밋이 나란히 설 수 있다
    fn place_commits(&mut self, graph: &GitGraph) {
        let is_parallel = graph.options.parallel_commits;
        if is_parallel && self.direction == Direction::BottomToTop {
            self.place_parallel_bottom_up(graph);
            return;
        }
        let start = if self.direction.is_vertical() {
            VERTICAL_START
        } else {
            0.0
        };
        let mut ordered: Vec<&Commit> = graph.commits.iter().collect();
        if self.direction == Direction::BottomToTop {
            ordered.reverse();
        }
        let mut pos = start;
        for commit in ordered {
            if is_parallel {
                pos = self.parallel_pos(commit, start);
            }
            self.set_commit(commit, pos, pos + LAYOUT_OFFSET);
            pos += COMMIT_STEP + LAYOUT_OFFSET;
            self.max_pos = self.max_pos.max(pos);
        }
    }

    /// 커밋 원 중심을 놓는다. LR 커밋은 브랜치 줄보다 2 위다
    fn set_commit(&mut self, commit: &Commit, pos: f64, along: f64) {
        let branch_pos = self.branch(&commit.branch).pos;
        let center = if self.direction.is_vertical() {
            Point::new(branch_pos, along)
        } else {
            Point::new(along, branch_pos - 2.0)
        };
        self.commits
            .insert(commit.id.clone(), CommitPlace { center, pos });
    }

    /// calculatePosition(LR·TB): 진행 방향으로 가장 먼 부모 + 40, 부모가 없으면 시작 자리
    fn parallel_pos(&self, commit: &Commit, start: f64) -> f64 {
        let is_vertical = self.direction.is_vertical();
        commit
            .parents
            .iter()
            .filter_map(|parent| self.commit(parent))
            .map(|place| {
                if is_vertical {
                    place.center.y
                } else {
                    place.center.x
                }
            })
            .reduce(f64::max)
            .map_or(start, |furthest| furthest + COMMIT_STEP)
    }

    /// setParallelBTPos와 그 뒤 calculatePosition(BT): 위에서 아래로 쌓은 자리를 거꾸로 접고, 마지막에 모두 40 올린다
    fn place_parallel_bottom_up(&mut self, graph: &GitGraph) {
        let mut heights: HashMap<&str, f64> = HashMap::new();
        let highest_parent = |heights: &HashMap<&str, f64>, commit: &Commit| {
            commit
                .parents
                .iter()
                .filter_map(|parent| heights.get(parent.as_str()).copied())
                .reduce(f64::min)
        };
        let (mut current, mut deepest) = (VERTICAL_START, VERTICAL_START);
        for commit in &graph.commits {
            if let Some(parent_y) = highest_parent(&heights, commit) {
                current = parent_y + COMMIT_STEP;
                deepest = deepest.max(current);
            }
            heights.insert(&commit.id, current + LAYOUT_OFFSET);
        }
        for root in graph
            .commits
            .iter()
            .filter(|commit| commit.parents.is_empty())
        {
            heights.insert(&root.id, deepest + VERTICAL_START);
        }
        for commit in &graph.commits {
            if let Some(parent_y) = highest_parent(&heights, commit) {
                heights.insert(&commit.id, parent_y - COMMIT_STEP - LAYOUT_OFFSET);
            }
        }
        for commit in graph.commits.iter().rev() {
            let pos = heights[commit.id.as_str()] - COMMIT_STEP;
            self.set_commit(commit, pos, pos);
            self.max_pos = self.max_pos.max(pos + COMMIT_STEP);
        }
    }

    pub fn branch(&self, name: &str) -> BranchPlace {
        self.branches
            .get(name)
            .copied()
            .unwrap_or(BranchPlace { pos: 0.0, index: 0 })
    }

    pub fn commit(&self, id: &str) -> Option<CommitPlace> {
        self.commits.get(id).copied()
    }

    /// 브랜치 줄 자리. LR은 커밋과 같이 2 위다
    pub fn spine(&self, name: &str) -> f64 {
        let pos = self.branch(name).pos;
        if self.direction.is_vertical() {
            pos
        } else {
            pos - 2.0
        }
    }

    pub fn reserve_lane(&mut self, lane: f64) {
        self.lanes.push(lane);
    }

    /// findLane: 두 자리의 가운데에서 시작해, 쓴 자리와 10 이상 떨어질 때까지 끝쪽을 1/5씩 당긴다
    fn find_lane(&mut self, start: f64, end: f64) -> f64 {
        let mut end = end;
        for _ in 0..=MAX_LANE_DEPTH {
            let candidate = start + (start - end).abs() / 2.0;
            if self
                .lanes
                .iter()
                .all(|lane| (lane - candidate).abs() >= LANE_GAP)
            {
                self.lanes.push(candidate);
                return candidate;
            }
            end -= (start - end).abs() / 5.0;
        }
        start + (start - end).abs() / 2.0
    }

    /// drawArrow: 부모 `from`에서 자식 `to`로 가는 경로. 사이에 같은 브랜치 커밋이 끼면 비어 있는 줄로 돌아간다
    pub fn arrow(&mut self, graph: &GitGraph, from: &Commit, to: &Commit) -> Option<ArrowPath> {
        let (start, end) = (self.commit(&from.id)?.center, self.commit(&to.id)?.center);
        let is_merge_side = to.kind == CommitKind::Merge && to.parents.first() != Some(&from.id);
        let mut color_branch = if is_merge_side {
            &from.branch
        } else {
            &to.branch
        };
        let vertical = self.direction.is_vertical();
        let (start_cross, end_cross) = if vertical {
            (start.x, end.x)
        } else {
            (start.y, end.y)
        };
        let path = if self.needs_reroute(graph, from, to, start_cross < end_cross) {
            if start_cross >= end_cross {
                color_branch = &from.branch;
            }
            let lane_y = self.find_lane(start.y.min(end.y), start.y.max(end.y));
            let lane_x = self.find_lane(start.x.min(end.x), start.x.max(end.x));
            let corners = if vertical {
                [Point::new(lane_x, start.y), Point::new(lane_x, end.y)]
            } else {
                [Point::new(start.x, lane_y), Point::new(end.x, lane_y)]
            };
            (
                vec![start, corners[0], corners[1], end],
                REROUTE_CORNER_RADIUS,
            )
        } else if start_cross == end_cross {
            (vec![start, end], NORMAL_CORNER_RADIUS)
        } else {
            (
                vec![start, self.corner(start, end, is_merge_side), end],
                NORMAL_CORNER_RADIUS,
            )
        };
        Some(ArrowPath {
            points: path.0,
            radius: path.1,
            color_index: self.branch(color_branch).index,
        })
    }

    /// 꺾임 하나짜리 경로의 모서리. LR은 줄을 먼저 건너고, 세로는 진행 방향으로 먼저 간다.
    /// 병합의 두 번째 부모 쪽은 그 반대다
    fn corner(&self, start: Point, end: Point, is_merge_side: bool) -> Point {
        let keeps_start_x = is_merge_side == self.direction.is_vertical();
        if keeps_start_x {
            Point::new(start.x, end.y)
        } else {
            Point::new(end.x, start.y)
        }
    }

    /// shouldRerouteArrow: 멀리 있는 쪽 브랜치에 두 커밋 사이 순번의 커밋이 있으면 돌아간다
    fn needs_reroute(
        &self,
        graph: &GitGraph,
        from: &Commit,
        to: &Commit,
        is_to_further: bool,
    ) -> bool {
        let curve_branch = if is_to_further {
            &to.branch
        } else {
            &from.branch
        };
        graph.commits.iter().any(|commit| {
            commit.seq > from.seq && commit.seq < to.seq && &commit.branch == curve_branch
        })
    }
}

/// drawText로 만든 브랜치 이름 글자의 getBBox 폭
pub fn branch_label_width(fonts: &FontBook, name: &str) -> f64 {
    let (mut left, mut right) = (0.0_f64, 0.0_f64);
    for row in branch_label_rows(name) {
        let extent = fonts.measure_svg_runs(&[&row], BRANCH_FONT_SIZE, FontWeight::Regular);
        left = left.min(extent.left);
        right = right.max(extent.right);
    }
    right - left
}

/// 이름표 글자 getBBox 높이. 줄마다 `dy="1em"`씩 내려가므로 한 줄 높이에 줄 수 − 1만큼 글자 크기를 더한다
pub fn branch_label_height(fonts: &FontBook, name: &str) -> f64 {
    let metrics = fonts.primary_metrics(BRANCH_FONT_SIZE);
    let extra_rows = branch_label_rows(name).len().saturating_sub(1) as f64;
    metrics.ascent + metrics.descent + extra_rows * BRANCH_FONT_SIZE
}

/// drawText: 브랜치 이름을 글자 그대로의 `\n`, 줄바꿈, `<br>`·`<br/>`(대소문자 무시)에서 나눠 앞뒤 공백을 지운 줄들
pub fn branch_label_rows(name: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row_start = 0;
    let mut index = 0;
    while index < name.len() {
        let rest = &name[index..];
        let separator_length = if rest.starts_with("\\n") {
            2
        } else if rest.starts_with('\n') {
            1
        } else {
            line_break_tag_length(rest)
        };
        if separator_length == 0 {
            index += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        }
        rows.push(name[row_start..index].trim().to_string());
        index += separator_length;
        row_start = index;
    }
    rows.push(name[row_start..].trim().to_string());
    rows
}

/// `<br\s*\/?>`(대소문자 무시)가 맨 앞에 있으면 그 길이, 없으면 0
fn line_break_tag_length(text: &str) -> usize {
    let is_open = text
        .get(..3)
        .is_some_and(|head| head.eq_ignore_ascii_case("<br"));
    if !is_open {
        return 0;
    }
    let after_name = &text[3..];
    let after_space = after_name.trim_start();
    let after_slash = after_space.strip_prefix('/').unwrap_or(after_space);
    match after_slash.strip_prefix('>') {
        Some(rest) => text.len() - rest.len(),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_graph::parse::parse;
    use crate::text::tests::bundled_fonts;

    #[test]
    fn 기준_svg와_같은_자리에_커밋과_브랜치를_둔다() {
        let graph = parse(
            "gitGraph\n commit\n commit\n branch develop\n checkout develop\n commit\n commit\n checkout main\n merge develop",
        )
        .unwrap();
        let fonts = bundled_fonts();
        let layout = GraphLayout::new(&graph, &fonts);
        assert_eq!(layout.branch("develop").pos, 90.0);
        let third = layout.commit(&graph.commits[2].id).unwrap();
        assert_eq!((third.center.x, third.center.y), (110.0, 88.0));
        assert_eq!(layout.max_pos, 250.0);
    }

    #[test]
    fn 아래에서_위로_나란히_쌓으면_mermaid_시험과_같은_자리에서_40_올린다() {
        // gitGraphRenderer.ts 'building BT parallel commit diagram' 시험과 같은 그래프
        let graph = parse(
            "%%{init: {'gitGraph': {'parallelCommits': true}}}%%\ngitGraph BT:\n commit id: \"1\"\n commit id: \"2\"\n branch develop\n commit id: \"3\"\n commit id: \"4\"\n checkout main\n branch feature\n commit id: \"5\"\n commit id: \"6\"\n checkout main\n commit id: \"7\"\n commit id: \"8\"",
        )
        .unwrap();
        let fonts = bundled_fonts();
        let layout = GraphLayout::new(&graph, &fonts);
        let heights: Vec<f64> = ["1", "2", "3", "4", "5", "6", "7", "8"]
            .iter()
            .map(|id| layout.commit(id).unwrap().center.y)
            .collect();
        assert_eq!(
            heights,
            vec![170.0, 120.0, 70.0, 20.0, 70.0, 20.0, 70.0, 20.0]
        );
    }

    #[test]
    fn 나란히_쌓으면_부모에서_50_뒤에_둔다() {
        let graph = parse(
            "%%{init: {'gitGraph': {'parallelCommits': true, 'rotateCommitLabel': false}}}%%\ngitGraph\n commit id: \"A\"\n branch dev\n commit id: \"B\"\n checkout main\n commit id: \"C\"",
        )
        .unwrap();
        let fonts = bundled_fonts();
        let layout = GraphLayout::new(&graph, &fonts);
        assert_eq!(layout.branch("dev").pos, 50.0);
        assert_eq!(layout.commit("B").unwrap().center.x, 60.0);
        assert_eq!(layout.commit("C").unwrap().center.x, 60.0);
        assert_eq!(layout.max_pos, 100.0);
    }

    #[test]
    fn 병합의_두_번째_부모_화살표는_가로로_간_뒤_올라간다() {
        let graph =
            parse("gitGraph\n commit\n branch develop\n commit\n checkout main\n merge develop")
                .unwrap();
        let fonts = bundled_fonts();
        let mut layout = GraphLayout::new(&graph, &fonts);
        let arrow = layout
            .arrow(&graph, &graph.commits[1], &graph.commits[2])
            .unwrap();
        assert_eq!(arrow.points[1], Point::new(110.0, 88.0));
        assert_eq!(arrow.color_index, 1);
    }

    #[test]
    fn 브랜치_이름은_br과_줄바꿈에서_줄을_나눈다() {
        assert_eq!(branch_label_rows("main"), vec!["main"]);
        assert_eq!(
            branch_label_rows("feature<br>login"),
            vec!["feature", "login"]
        );
        assert_eq!(
            branch_label_rows("a<BR />b\\nc\nd"),
            vec!["a", "b", "c", "d"]
        );
        assert_eq!(branch_label_rows("x <br/ > y"), vec!["x <br/ > y"]);
    }

    #[test]
    fn 여러_줄_이름표는_줄마다_글자_크기만큼_높다() {
        // 기준 SVG gitgraph-multiline-branch: 한 줄 바탕 높이 26, 두 줄 42(글자 높이 + 4)
        let fonts = bundled_fonts();
        assert_eq!(branch_label_height(&fonts, "main") + 4.0, 26.0);
        assert_eq!(branch_label_height(&fonts, "feature<br>login") + 4.0, 42.0);
    }
}
