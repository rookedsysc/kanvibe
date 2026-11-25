//! treeView-beta 다이어그램(treeView/db.ts·renderer.ts). 들여쓰기로 정한 폴더 나무를 한 줄에 한 이름씩 그린다.
//!
//! 이름은 따옴표 글자이고, 들여쓰기(공백·탭 글자 수)가 앞 이름보다 깊으면 그 아래로 들어간다. 맨 위에는 이름이
//! `/`인 뿌리가 있다. 이름 왼쪽에 가로 가지, 자식이 있는 이름 아래에 마지막 자식 가운데까지 세로 줄기를 긋는다.

use crate::directive::diagram_body;
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, TextItem};
use crate::text::FontWeight;
use crate::{DiagramError, RenderContext};

/// defaultConfig.treeView
const ROW_INDENT: f64 = 10.0;
const PADDING_X: f64 = 5.0;
const PADDING_Y: f64 = 5.0;
const LINE_THICKNESS: f64 = 1.0;
const FONT_SIZE: f64 = 16.0;
/// Noto Sans x-height(1em 기준). `dominant-baseline: middle`은 기준선을 x-height 절반만큼 내린다
const X_HEIGHT_RATIO: f64 = 0.536;
/// treeView/styles.ts: 글자와 선은 테마와 상관없이 검정이다
const INK: Color = Color::rgb(0, 0, 0);

#[derive(Clone, Debug, PartialEq)]
struct TreeNode {
    name: String,
    children: Vec<TreeNode>,
}

fn parse(definition: &str) -> Result<TreeNode, DiagramError> {
    let body = diagram_body(definition);
    let mut lines = body
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("%%"));
    if lines.next().map(str::trim) != Some("treeView-beta") {
        return Err(DiagramError::Syntax("treeView-beta 머리줄".to_string()));
    }
    let mut entries = Vec::new();
    for line in lines {
        let name = line.trim();
        if ["title", "accTitle", "accDescr"]
            .iter()
            .any(|keyword| name.starts_with(keyword))
        {
            continue;
        }
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        entries.push((indent, unquote(name)?));
    }
    Ok(build_tree(entries))
}

/// STRING2: `"이름"` 또는 `'이름'`
fn unquote(text: &str) -> Result<String, DiagramError> {
    let quote = text.chars().next().filter(|c| *c == '"' || *c == '\'');
    quote
        .and_then(|quote| {
            let inner = text.strip_prefix(quote)?.strip_suffix(quote)?;
            (!inner.contains(quote)).then_some(inner)
        })
        .map(str::to_string)
        .ok_or_else(|| DiagramError::Syntax(format!("treeView 이름 형식: {text}")))
}

/// db.addNode: 들여쓰기가 같거나 얕은 조상을 쌓기에서 걷어 낸 뒤 맨 위 조상의 자식으로 넣는다
fn build_tree(entries: Vec<(usize, String)>) -> TreeNode {
    let root = TreeNode {
        name: "/".to_string(),
        children: Vec::new(),
    };
    // 뿌리의 들여쓰기는 -1이라 쌓기에서 빠지지 않는다
    let mut stack: Vec<(i64, TreeNode)> = vec![(-1, root)];
    for (indent, name) in entries {
        let level = indent as i64;
        while stack.len() > 1 && level <= stack[stack.len() - 1].0 {
            close_top(&mut stack);
        }
        let node = TreeNode {
            name,
            children: Vec::new(),
        };
        stack.push((level, node));
    }
    while stack.len() > 1 {
        close_top(&mut stack);
    }
    stack
        .pop()
        .map(|(_, node)| node)
        .unwrap_or_else(|| TreeNode {
            name: "/".to_string(),
            children: Vec::new(),
        })
}

fn close_top(stack: &mut Vec<(i64, TreeNode)>) {
    if let Some((_, node)) = stack.pop()
        && let Some((_, parent)) = stack.last_mut()
    {
        parent.children.push(node);
    }
}

/// 그린 이름 하나의 상자(positionLabel의 node.BBox)
#[derive(Clone, Copy)]
struct RowBox {
    x: f64,
    y: f64,
    height: f64,
}

struct TreePainter<'a> {
    context: &'a RenderContext<'a>,
    scene: Scene,
    total_height: f64,
    total_width: f64,
}

impl TreePainter<'_> {
    /// processNode: 자기 줄을 그리고 자식을 그린 뒤, 자식이 있으면 세로 줄기를 긋는다
    fn draw_subtree(&mut self, node: &TreeNode, depth: usize) -> RowBox {
        let row = self.draw_row(node, depth);
        let mut last_child = None;
        for child in &node.children {
            last_child = Some(self.draw_subtree(child, depth + 1));
        }
        if let Some(last) = last_child {
            let x = row.x + PADDING_X;
            let end_y = last.y + last.height / 2.0 + LINE_THICKNESS / 2.0;
            self.push_line(Point::new(x, row.y + row.height), Point::new(x, end_y));
        }
        row
    }

    /// drawNode: getBBox 높이에 위아래 여백을 더한 줄에 이름을 놓고 왼쪽에 가로 가지를 긋는다
    fn draw_row(&mut self, node: &TreeNode, depth: usize) -> RowBox {
        let fonts = self.context.fonts;
        let indent = depth as f64 * (ROW_INDENT + PADDING_X);
        let extent = fonts.measure_svg_runs(&[node.name.as_str()], FONT_SIZE, FontWeight::Regular);
        let metrics = fonts.primary_metrics(FONT_SIZE);
        let height = metrics.ascent + metrics.descent + PADDING_Y * 2.0;
        let width = extent.width() + PADDING_X * 2.0;
        let y = self.total_height;
        self.scene.push_text(TextItem {
            text: node.name.clone(),
            x: indent + PADDING_X,
            baseline: y + height / 2.0 + X_HEIGHT_RATIO * FONT_SIZE / 2.0,
            font_size: FONT_SIZE,
            weight: FontWeight::Regular,
            color: INK,
            rotation: None,
        });
        let middle = y + height / 2.0;
        self.push_line(
            Point::new(indent - ROW_INDENT, middle),
            Point::new(indent, middle),
        );
        self.total_width = self.total_width.max(indent + width);
        self.total_height += height;
        RowBox {
            x: indent,
            y,
            height,
        }
    }

    fn push_line(&mut self, from: Point, to: Point) {
        let stroke = StrokeStyle {
            color: INK,
            width: LINE_THICKNESS,
            dash: Vec::new(),
        };
        self.scene.push_path(
            vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)],
            None,
            Some(stroke),
        );
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let root = parse(definition)?;
    let mut painter = TreePainter {
        context,
        scene: Scene::default(),
        total_height: 0.0,
        total_width: 0.0,
    };
    painter.draw_subtree(&root, 0);
    let mut scene = painter.scene;
    scene.width = painter.total_width;
    scene.height = painter.total_height;
    // viewBox `-선굵기/2 0 폭 높이`
    scene.translate(LINE_THICKNESS / 2.0, 0.0);
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LabelMode;
    use crate::text::tests::bundled_fonts;
    use crate::theme::{Palette, Theme};

    fn names(node: &TreeNode) -> Vec<String> {
        node.children
            .iter()
            .map(|child| child.name.clone())
            .collect()
    }

    #[test]
    fn 들여쓰기로_부모를_정한다() {
        let root =
            parse("treeView-beta\n\"docs\"\n  \"build\"\n  'source'\n    \"conf.py\"\n\"src\"")
                .unwrap();
        assert_eq!(root.name, "/");
        assert_eq!(names(&root), vec!["docs", "src"]);
        assert_eq!(names(&root.children[0]), vec!["build", "source"]);
        assert_eq!(names(&root.children[0].children[1]), vec!["conf.py"]);
    }

    #[test]
    fn 따옴표_없는_이름은_문법_오류다() {
        assert!(parse("treeView-beta\n  docs").is_err());
    }

    #[test]
    fn 기준_svg와_같은_폭과_높이를_만든다() {
        // treeview-basic 기준 SVG: viewBox -0.5 0 120.584 288(줄 높이 32, index.rst가 가장 오른쪽)
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::Html,
        };
        let definition = "treeView-beta\n\"docs\"\n  \"build\"\n  \"make.bat\"\n  \"source\"\n    \"conf.py\"\n    \"index.rst\"\n\"src\"\n  \"main.rs\"";
        let scene = render(definition, &context).unwrap();
        assert_eq!(scene.height, 288.0);
        assert!((scene.width - 120.584).abs() < 0.01, "{}", scene.width);
    }
}
