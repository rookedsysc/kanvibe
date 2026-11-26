//! mindmap.jison과 mindmapDb.ts의 노드 추가 규칙. 들여쓰기 칸 수가 노드 깊이다.

use crate::DiagramError;

/// mindmapDb `nodeType`. 테두리 없는 기본형은 `defaultMindmapNode`로 그린다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeShape {
    Default,
    RoundedRect,
    Rect,
    Circle,
    Cloud,
    Bang,
    Hexagon,
}

#[derive(Clone, Debug)]
pub(crate) struct MindmapNode {
    /// 루트 들여쓰기를 뺀 들여쓰기 칸 수(루트는 0)
    pub level: usize,
    pub label: String,
    /// `::icon(...)` 꾸밈이 붙었는지. 아이콘 글꼴은 그리지 않지만 SVG 글자 라벨에 배경 여백이 생긴다
    pub has_icon: bool,
    pub shape: NodeShape,
    pub children: Vec<usize>,
    /// 루트 바로 아래 가지 순번(루트는 `None`)
    pub section: Option<usize>,
}

/// Mermaid `MAX_SECTIONS - 1`. 가지 색은 이 수만큼 돌아가며 쓴다
const SECTION_COUNT: usize = 11;

/// 노드를 정의 순서(= 앞선 깊이 우선 순서)로 돌려준다. 0번이 루트다
pub(crate) fn parse(definition: &str) -> Result<Vec<MindmapNode>, DiagramError> {
    let mut nodes: Vec<MindmapNode> = Vec::new();
    let mut base_level: usize = 0;
    let body = crate::directive::diagram_body(definition);
    let mut lines = body.lines().filter(|line| !is_skipped(line));
    let header = lines.next().unwrap_or_default();
    if header.trim() != "mindmap" {
        return Err(DiagramError::Syntax("mindmap 머리줄".to_string()));
    }
    for line in lines {
        let indent = line.len() - line.trim_start().len();
        let content = strip_class(line.trim());
        if content.starts_with("::icon(") {
            if let Some(last) = nodes.last_mut() {
                last.has_icon = true;
            }
            continue;
        }
        if content.is_empty() {
            continue;
        }
        let (label, shape) = parse_node(content);
        let level = if nodes.is_empty() {
            base_level = indent;
            0
        } else {
            indent.saturating_sub(base_level)
        };
        attach_node(&mut nodes, level, label, shape)?;
    }
    if nodes.is_empty() {
        return Err(DiagramError::Syntax("mindmap 노드가 없습니다".to_string()));
    }
    assign_sections(&mut nodes);
    Ok(nodes)
}

fn is_skipped(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with("%%")
}

/// `:::클래스` 꾸밈은 색에 쓰지 않으므로 떼어 낸다
fn strip_class(content: &str) -> &str {
    match content.find(":::") {
        Some(index) => content[..index].trim_end(),
        None => content,
    }
}

/// mindmapDb.getParent: 자기보다 얕은 마지막 노드가 부모다
fn attach_node(
    nodes: &mut Vec<MindmapNode>,
    level: usize,
    label: String,
    shape: NodeShape,
) -> Result<(), DiagramError> {
    let index = nodes.len();
    let parent = nodes.iter().rposition(|node| node.level < level);
    match parent {
        Some(parent) => nodes[parent].children.push(index),
        None if nodes.is_empty() => {}
        None => {
            return Err(DiagramError::Syntax(format!(
                "There can be only one root. No parent could be found for (\"{label}\")"
            )));
        }
    }
    nodes.push(MindmapNode {
        level,
        label,
        has_icon: false,
        shape,
        children: Vec::new(),
        section: None,
    });
    Ok(())
}

/// 루트 자식마다 가지 번호를 매기고 자손에게 물려준다
fn assign_sections(nodes: &mut [MindmapNode]) {
    let root_children = nodes[0].children.clone();
    for (index, child) in root_children.into_iter().enumerate() {
        let mut stack = vec![child];
        while let Some(node) = stack.pop() {
            nodes[node].section = Some(index % SECTION_COUNT);
            stack.extend(nodes[node].children.iter().copied());
        }
    }
}

/// `id[글]`, `id(글)`, `id((글))`, `id))글((`, `id)글(`, `id{{글}}`, 그냥 `글`
fn parse_node(content: &str) -> (String, NodeShape) {
    let Some(open_index) = content.find(['(', '[', '{', ')']) else {
        return (content.to_string(), NodeShape::Default);
    };
    let rest = &content[open_index..];
    let opener = ["((", "))", "{{", "(", ")", "["]
        .into_iter()
        .find(|token| rest.starts_with(token))
        .unwrap_or("");
    if opener.is_empty() {
        return (content.to_string(), NodeShape::Default);
    }
    let inner = &rest[opener.len()..];
    let closer = ["))", "((", "}}", ")", "(", "]"]
        .into_iter()
        .find(|token| inner.ends_with(token))
        .unwrap_or("");
    let description = &inner[..inner.len() - closer.len()];
    (unquote(description), shape_for(opener, closer))
}

/// mindmapDb.getType
fn shape_for(opener: &str, closer: &str) -> NodeShape {
    match opener {
        "[" => NodeShape::Rect,
        "(" if closer == ")" => NodeShape::RoundedRect,
        "(" | ")" => NodeShape::Cloud,
        "((" => NodeShape::Circle,
        "))" => NodeShape::Bang,
        "{{" => NodeShape::Hexagon,
        _ => NodeShape::Default,
    }
}

/// `"글"`과 마크다운 문자열 `"`글`"`의 따옴표를 벗긴다
fn unquote(description: &str) -> String {
    let trimmed = description.trim();
    let without_quotes = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(trimmed);
    let without_markdown = without_quotes
        .strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
        .unwrap_or(without_quotes);
    without_markdown.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 들여쓰기로_나무와_가지를_만든다() {
        let nodes =
            parse("mindmap\n  root((KanVibe))\n    A\n      B[Box]\n    C{{Hex}}\n").unwrap();
        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].shape, NodeShape::Circle);
        assert_eq!(nodes[0].label, "KanVibe");
        assert_eq!(nodes[0].children, vec![1, 3]);
        assert_eq!(nodes[2].level, 4);
        assert_eq!(nodes[2].shape, NodeShape::Rect);
        assert_eq!(nodes[2].section, Some(0));
        assert_eq!(nodes[3].section, Some(1));
        assert_eq!(nodes[3].shape, NodeShape::Hexagon);
    }

    #[test]
    fn 괄호_모양을_구분한다() {
        assert_eq!(parse_node("a(b)").1, NodeShape::RoundedRect);
        assert_eq!(parse_node("a)b(").1, NodeShape::Cloud);
        assert_eq!(parse_node("a))b((").1, NodeShape::Bang);
        assert_eq!(parse_node("plain text").1, NodeShape::Default);
    }
}
