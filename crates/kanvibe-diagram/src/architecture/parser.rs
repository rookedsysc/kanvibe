//! architecture 문법(@mermaid-js/parser `ArchitectureGrammar`)과 architectureDb.ts의 추가 규칙.
//!
//! 한 줄이 한 문장이다. `group`·`service`·`junction` 선언과 `a:R -- L:b` 꼴 간선만 읽고, `title`·`accTitle`·
//! `accDescr`는 그리지 않으므로 건너뛴다. 간선은 두 끝이 이미 선언된 노드여야 한다(addEdge).

use crate::DiagramError;
use crate::directive::diagram_body;

/// 노드 한 변(ArchitectureDirection)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn parse(token: &str) -> Option<Self> {
        match token {
            "L" => Some(Self::Left),
            "R" => Some(Self::Right),
            "T" => Some(Self::Top),
            "B" => Some(Self::Bottom),
            _ => None,
        }
    }

    /// `L`·`R`이면 참(isArchitectureDirectionX)
    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    pub fn letter(self) -> char {
        match self {
            Self::Left => 'L',
            Self::Right => 'R',
            Self::Top => 'T',
            Self::Bottom => 'B',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Service,
    Junction,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArchitectureGroup {
    pub id: String,
    pub icon: Option<String>,
    pub title: Option<String>,
    pub parent: Option<String>,
}

/// 서비스나 접점(junction)
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArchitectureNode {
    pub id: String,
    pub kind: NodeKind,
    pub icon: Option<String>,
    /// 아이콘 대신 쓰는 글자(`service id "글자"`)
    pub icon_text: Option<String>,
    pub title: Option<String>,
    pub parent: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArchitectureEdge {
    pub source: String,
    pub source_side: Side,
    pub has_source_arrow: bool,
    /// `{group}`: 끝을 그룹 테두리 밖으로 뺀다
    pub is_source_group: bool,
    pub target: String,
    pub target_side: Side,
    pub has_target_arrow: bool,
    pub is_target_group: bool,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Architecture {
    pub groups: Vec<ArchitectureGroup>,
    pub nodes: Vec<ArchitectureNode>,
    pub edges: Vec<ArchitectureEdge>,
}

impl Architecture {
    pub fn node(&self, id: &str) -> Option<&ArchitectureNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|node| node.id == id)
    }

    fn is_registered(&self, id: &str) -> bool {
        self.node(id).is_some() || self.groups.iter().any(|group| group.id == id)
    }
}

pub(crate) fn parse(definition: &str) -> Result<Architecture, DiagramError> {
    let body = diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    if lines.next() != Some("architecture-beta") {
        return Err(syntax("architecture-beta 머리줄"));
    }
    let mut architecture = Architecture::default();
    for line in lines {
        parse_statement(&mut architecture, strip_comment(line))?;
    }
    Ok(architecture)
}

fn syntax(message: &str) -> DiagramError {
    DiagramError::Syntax(message.to_string())
}

fn strip_comment(line: &str) -> &str {
    line.find("%%")
        .map_or(line, |index| line[..index].trim_end())
}

fn parse_statement(architecture: &mut Architecture, line: &str) -> Result<(), DiagramError> {
    let keyword = line.split_whitespace().next().unwrap_or_default();
    match keyword {
        "" | "title" => Ok(()),
        _ if line.starts_with("accTitle") || line.starts_with("accDescr") => Ok(()),
        "group" => {
            let group = parse_group(&line[keyword.len()..])?;
            register_check(architecture, &group.id, group.parent.as_deref())?;
            architecture.groups.push(group);
            Ok(())
        }
        "service" | "junction" => {
            let kind = if keyword == "service" {
                NodeKind::Service
            } else {
                NodeKind::Junction
            };
            let node = parse_node(&line[keyword.len()..], kind)?;
            register_check(architecture, &node.id, node.parent.as_deref())?;
            architecture.nodes.push(node);
            Ok(())
        }
        _ => {
            let edge = parse_edge(line)?;
            check_edge(architecture, &edge)?;
            architecture.edges.push(edge);
            Ok(())
        }
    }
}

/// addGroup·addService·addJunction: 아이디는 한 번만, 부모는 먼저 선언된 그룹이어야 한다
fn register_check(
    architecture: &Architecture,
    id: &str,
    parent: Option<&str>,
) -> Result<(), DiagramError> {
    if architecture.is_registered(id) {
        return Err(syntax(&format!("이미 쓰인 아이디 {id}")));
    }
    let Some(parent) = parent else {
        return Ok(());
    };
    if parent == id || !architecture.groups.iter().any(|group| group.id == parent) {
        return Err(syntax(&format!("{id}의 부모 그룹 {parent}이 없습니다")));
    }
    Ok(())
}

fn check_edge(architecture: &Architecture, edge: &ArchitectureEdge) -> Result<(), DiagramError> {
    let (Some(source), Some(target)) = (
        architecture.node(&edge.source),
        architecture.node(&edge.target),
    ) else {
        return Err(syntax("간선 끝 노드가 선언되지 않았습니다"));
    };
    let is_same_group = source.parent.is_some() && source.parent == target.parent;
    if is_same_group && (edge.is_source_group || edge.is_target_group) {
        return Err(syntax("{group} 간선은 두 그룹을 지나야 합니다"));
    }
    // cytoscape 간선 아이디 `왼쪽-오른쪽`은 노드·그룹·다른 간선 아이디와 겹치면 add가 실패한다
    let edge_id = format!("{}-{}", edge.source, edge.target);
    let is_taken = architecture.is_registered(&edge_id)
        || architecture
            .edges
            .iter()
            .any(|other| format!("{}-{}", other.source, other.target) == edge_id);
    if is_taken {
        return Err(syntax(&format!("간선 아이디 {edge_id}가 겹칩니다")));
    }
    Ok(())
}

/// 앞에서부터 낱말을 하나씩 떼는 작은 읽개
struct Cursor<'a> {
    rest: &'a str,
}

impl<'a> Cursor<'a> {
    fn new(text: &'a str) -> Self {
        Self { rest: text.trim() }
    }

    /// ID 터미널 `[\w]([-\w]*\w)?`
    fn identifier(&mut self) -> Option<&'a str> {
        let length = self
            .rest
            .char_indices()
            .find(|(_, character)| {
                !(character.is_alphanumeric() || *character == '_' || *character == '-')
            })
            .map_or(self.rest.len(), |(index, _)| index);
        let token = self.rest[..length].trim_end_matches('-');
        if token.is_empty() {
            return None;
        }
        self.rest = self.rest[token.len()..].trim_start();
        Some(token)
    }

    /// `open`으로 시작해 `close`로 끝나는 토막의 안쪽
    fn enclosed(&mut self, open: char, close: char) -> Option<&'a str> {
        let inner = self.rest.strip_prefix(open)?;
        let end = inner.find(close)?;
        self.rest = inner[end + close.len_utf8()..].trim_start();
        Some(&inner[..end])
    }

    fn quoted(&mut self) -> Option<&'a str> {
        let quote = self
            .rest
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')?;
        self.enclosed(quote, quote)
    }

    fn keyword(&mut self, word: &str) -> bool {
        match self.rest.strip_prefix(word) {
            Some(after) => {
                self.rest = after.trim_start();
                true
            }
            None => false,
        }
    }

    fn is_done(&self) -> bool {
        self.rest.is_empty()
    }
}

/// ARCH_TITLE 값 변환: 대괄호와 둘레 따옴표를 벗긴다
fn title_value(raw: &str) -> String {
    let trimmed = raw.trim();
    let is_quoted = trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')));
    if is_quoted {
        let inner = &trimmed[1..trimmed.len() - 1];
        return inner
            .replace("\\\"", "\"")
            .replace("\\'", "'")
            .trim()
            .to_string();
    }
    trimmed.to_string()
}

fn parse_parent(cursor: &mut Cursor) -> Result<Option<String>, DiagramError> {
    if !cursor.keyword("in ") {
        return Ok(None);
    }
    let parent = cursor
        .identifier()
        .ok_or_else(|| syntax("in 뒤 그룹 아이디"))?;
    Ok(Some(parent.to_string()))
}

fn parse_group(text: &str) -> Result<ArchitectureGroup, DiagramError> {
    let mut cursor = Cursor::new(text);
    let id = cursor.identifier().ok_or_else(|| syntax("그룹 아이디"))?;
    let icon = cursor
        .enclosed('(', ')')
        .map(|icon| icon.trim().to_string());
    let title = cursor.enclosed('[', ']').map(title_value);
    let parent = parse_parent(&mut cursor)?;
    if !cursor.is_done() {
        return Err(syntax("그룹 선언 뒤 남은 글자"));
    }
    Ok(ArchitectureGroup {
        id: id.to_string(),
        icon,
        title,
        parent,
    })
}

fn parse_node(text: &str, kind: NodeKind) -> Result<ArchitectureNode, DiagramError> {
    let mut cursor = Cursor::new(text);
    let id = cursor.identifier().ok_or_else(|| syntax("노드 아이디"))?;
    let mut node = ArchitectureNode {
        id: id.to_string(),
        kind,
        icon: None,
        icon_text: None,
        title: None,
        parent: None,
    };
    if kind == NodeKind::Service {
        node.icon_text = cursor.quoted().map(str::to_string);
        if node.icon_text.is_none() {
            node.icon = cursor
                .enclosed('(', ')')
                .map(|icon| icon.trim().to_string());
        }
        node.title = cursor.enclosed('[', ']').map(title_value);
    }
    node.parent = parse_parent(&mut cursor)?;
    if !cursor.is_done() {
        return Err(syntax("노드 선언 뒤 남은 글자"));
    }
    Ok(node)
}

/// 간선 한쪽 끝: `id{group}:R`(왼쪽) 또는 `L:id{group}`(오른쪽)
fn parse_endpoint_group(cursor: &mut Cursor) -> bool {
    cursor.keyword("{group}")
}

fn parse_side(cursor: &mut Cursor) -> Result<Side, DiagramError> {
    let letter = cursor.rest.get(..1).and_then(Side::parse);
    let side = letter.ok_or_else(|| syntax("간선 방향(L·R·T·B)"))?;
    cursor.rest = cursor.rest[1..].trim_start();
    Ok(side)
}

fn parse_edge(line: &str) -> Result<ArchitectureEdge, DiagramError> {
    let mut cursor = Cursor::new(line);
    let source = cursor
        .identifier()
        .ok_or_else(|| syntax("간선 왼쪽 아이디"))?;
    let is_source_group = parse_endpoint_group(&mut cursor);
    if !cursor.keyword(":") {
        return Err(syntax("간선 왼쪽 ':'"));
    }
    let source_side = parse_side(&mut cursor)?;
    let has_source_arrow = cursor.keyword("<");
    let title = parse_arrow_body(&mut cursor)?;
    let has_target_arrow = cursor.keyword(">");
    let target_side = parse_side(&mut cursor)?;
    if !cursor.keyword(":") {
        return Err(syntax("간선 오른쪽 ':'"));
    }
    let target = cursor
        .identifier()
        .ok_or_else(|| syntax("간선 오른쪽 아이디"))?;
    let is_target_group = parse_endpoint_group(&mut cursor);
    if !cursor.is_done() {
        return Err(syntax("간선 뒤 남은 글자"));
    }
    Ok(ArchitectureEdge {
        source: source.to_string(),
        source_side,
        has_source_arrow,
        is_source_group,
        target: target.to_string(),
        target_side,
        has_target_arrow,
        is_target_group,
        title,
    })
}

/// `--` 또는 `-[제목]-`
fn parse_arrow_body(cursor: &mut Cursor) -> Result<Option<String>, DiagramError> {
    if cursor.keyword("--") {
        return Ok(None);
    }
    if !cursor.keyword("-") {
        return Err(syntax("간선 선(--)"));
    }
    let title = cursor
        .enclosed('[', ']')
        .map(title_value)
        .ok_or_else(|| syntax("간선 제목"))?;
    if !cursor.keyword("-") {
        return Err(syntax("간선 제목 뒤 '-'"));
    }
    Ok(Some(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 그룹_서비스_간선을_읽는다() {
        let architecture = parse(
            "architecture-beta\n  group api(cloud)[API]\n  service db(database)[Database] in api\n  service server(server)[Server] in api\n  db:L -- R:server\n",
        )
        .unwrap();
        assert_eq!(architecture.groups[0].icon.as_deref(), Some("cloud"));
        assert_eq!(architecture.nodes[0].parent.as_deref(), Some("api"));
        assert_eq!(architecture.nodes[1].title.as_deref(), Some("Server"));
        let edge = &architecture.edges[0];
        assert_eq!(
            (edge.source_side, edge.target_side),
            (Side::Left, Side::Right)
        );
        assert!(!edge.has_source_arrow && !edge.has_target_arrow);
    }

    #[test]
    fn 화살표_그룹_꾸밈_제목을_읽는다() {
        let architecture = parse(
            "architecture-beta\n group a\n group b\n service x in a\n service y in b\n junction j in b\n x{group}:R --> L:y{group}\n y:R -[reads]- L:j\n j:B <-- T:x\n",
        )
        .unwrap();
        let first = &architecture.edges[0];
        assert!(first.is_source_group && first.is_target_group && first.has_target_arrow);
        assert_eq!(architecture.edges[1].title.as_deref(), Some("reads"));
        assert!(architecture.edges[2].has_source_arrow);
        assert_eq!(architecture.nodes[2].kind, NodeKind::Junction);
    }

    #[test]
    fn 아이콘_접두어와_따옴표_제목을_읽는다() {
        let architecture =
            parse("architecture-beta\n title 제목\n service l(logos:aws-lambda)[\"Lambda fn\"]\n")
                .unwrap();
        let node = &architecture.nodes[0];
        assert_eq!(node.icon.as_deref(), Some("logos:aws-lambda"));
        assert_eq!(node.title.as_deref(), Some("Lambda fn"));
    }

    #[test]
    fn 선언되지_않은_간선_끝은_오류다() {
        assert!(parse("architecture-beta\n service a\n a:L -- R:b\n").is_err());
    }

    #[test]
    fn 같은_방향_두_간선은_cytoscape_아이디가_겹쳐_오류다() {
        let repeated = "architecture-beta\n service a\n service b\n a:R -- L:b\n a:B -- T:b\n";
        assert!(parse(repeated).is_err());
        let reversed = "architecture-beta\n service a\n service b\n a:R -- L:b\n b:B -- T:a\n";
        assert!(parse(reversed).is_ok());
    }
}
