//! flowchart·graph 정의 파서. Mermaid flow.jison의 토큰 규칙과 FlowDB의 저장 순서를 따른다.
//!
//! 지원: 방향, 노드 모양 14종, 링크(실선·점선·굵은선·숨김, 화살표·원·X, 양방향, 길이), 파이프·인라인 라벨,
//! `&` 묶음, subgraph(제목·direction·중첩), classDef·class·`:::`·style·linkStyle, `%%` 주석.
//! 지원하지 않는 문장(`click`, `@{ shape }` 등)은 건너뛰지 않고 문법 오류로 돌려 기존 앱처럼 원문 표시로 넘긴다.

use std::collections::HashMap;

use super::render::CustomMarker;
use crate::DiagramError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    TopBottom,
    BottomTop,
    LeftRight,
    RightLeft,
}

impl Direction {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "TB" | "TD" => Some(Self::TopBottom),
            "BT" => Some(Self::BottomTop),
            "LR" => Some(Self::LeftRight),
            "RL" => Some(Self::RightLeft),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeShape {
    Rect,
    Rounded,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    DoubleCircle,
    Diamond,
    Hexagon,
    LeanRight,
    LeanLeft,
    Trapezoid,
    InvTrapezoid,
    Odd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowType {
    None,
    Point,
    Circle,
    Cross,
    /// stateDiagram 전이 끝의 barb 화살표(flowchart 문법에는 없다)
    Barb,
    /// `look: neo` stateDiagram 전이 끝의 barbEnd-margin 화살표(채운 화살촉, 선은 5.5 당긴다)
    BarbNeo,
    /// 다른 다이어그램이 정의한 표식(erDiagram 관계 차수, requirementDiagram 화살표 등)
    Custom(&'static CustomMarker),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stroke {
    Normal,
    Thick,
    Dotted,
    Invisible,
    /// stateDiagram 메모 간선의 `stroke-dasharray: 5`(flowchart 문법에는 없다)
    Dashed,
}

#[derive(Clone, Debug)]
pub struct Vertex {
    pub id: String,
    pub text: String,
    pub shape: NodeShape,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub start: String,
    pub end: String,
    pub text: String,
    pub arrow_start: ArrowType,
    pub arrow_end: ArrowType,
    pub stroke: Stroke,
    pub length: i32,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Subgraph {
    pub id: String,
    pub title: String,
    pub nodes: Vec<String>,
    pub dir: Option<Direction>,
}

#[derive(Clone, Debug)]
pub struct Flowchart {
    pub direction: Direction,
    /// 처음 나온 순서
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    /// FlowDB처럼 닫힌(`end`) 순서. 안쪽 subgraph가 먼저 온다
    pub subgraphs: Vec<Subgraph>,
    pub class_styles: HashMap<String, Vec<String>>,
}

struct OpenSubgraph {
    id: Option<String>,
    title: String,
    members: Vec<String>,
    dir: Option<Direction>,
}

struct Parser {
    chart: Flowchart,
    vertex_index: HashMap<String, usize>,
    open_subgraphs: Vec<OpenSubgraph>,
    subgraph_count: usize,
    default_link_style: Vec<String>,
}

fn syntax(message: impl Into<String>) -> DiagramError {
    DiagramError::Syntax(message.into())
}

pub fn parse(definition: &str) -> Result<Flowchart, DiagramError> {
    let mut lines = definition.lines().map(strip_comment);
    let header = lines
        .by_ref()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| syntax("빈 정의"))?;
    let (direction, rest) = parse_header(header)?;
    let mut parser = Parser {
        chart: Flowchart {
            direction,
            vertices: Vec::new(),
            edges: Vec::new(),
            subgraphs: Vec::new(),
            class_styles: HashMap::new(),
        },
        vertex_index: HashMap::new(),
        open_subgraphs: Vec::new(),
        subgraph_count: 0,
        default_link_style: Vec::new(),
    };
    for statement in split_statements(rest)
        .into_iter()
        .chain(lines.flat_map(split_statements))
    {
        parser.statement(statement.trim())?;
    }
    if !parser.open_subgraphs.is_empty() {
        return Err(syntax("subgraph가 end로 닫히지 않았습니다"));
    }
    if !parser.default_link_style.is_empty() {
        for edge in &mut parser.chart.edges {
            if edge.styles.is_empty() {
                edge.styles = parser.default_link_style.clone();
            }
        }
    }
    Ok(parser.chart)
}

fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with("%%") {
        ""
    } else {
        line
    }
}

fn parse_header(line: &str) -> Result<(Direction, &str), DiagramError> {
    let trimmed = line.trim();
    let rest = trimmed
        .strip_prefix("flowchart")
        .or_else(|| trimmed.strip_prefix("graph"))
        .ok_or_else(|| syntax("flowchart 머리줄이 아닙니다"))?;
    let rest = rest.trim_start();
    let token_end = rest
        .find(|character: char| character.is_whitespace() || character == ';')
        .unwrap_or(rest.len());
    match Direction::parse(&rest[..token_end]) {
        Some(direction) => Ok((direction, &rest[token_end..])),
        None if token_end == 0 => Ok((Direction::TopBottom, rest)),
        None => Err(syntax(format!("알 수 없는 방향: {}", &rest[..token_end]))),
    }
}

/// 따옴표·괄호·파이프 밖의 `;`로 문장을 나눈다
fn split_statements(line: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut depth = 0i32;
    let mut is_quoted = false;
    let mut is_piped = false;
    let mut start = 0;
    for (index, character) in line.char_indices() {
        match character {
            '"' => is_quoted = !is_quoted,
            '|' if !is_quoted => is_piped = !is_piped,
            '[' | '(' | '{' if !is_quoted => depth += 1,
            ']' | ')' | '}' if !is_quoted => depth -= 1,
            ';' if !is_quoted && !is_piped && depth <= 0 => {
                statements.push(&line[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    statements.push(&line[start..]);
    statements
}

fn keyword_rest<'a>(statement: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = statement.strip_prefix(keyword)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}

impl Parser {
    fn statement(&mut self, statement: &str) -> Result<(), DiagramError> {
        if statement.is_empty() {
            return Ok(());
        }
        if let Some(rest) = keyword_rest(statement, "subgraph") {
            self.open_subgraph(rest);
            return Ok(());
        }
        if statement == "end" {
            return self.close_subgraph();
        }
        if let Some(rest) = keyword_rest(statement, "direction") {
            let direction = Direction::parse(rest).ok_or_else(|| syntax("알 수 없는 방향"))?;
            if let Some(open) = self.open_subgraphs.last_mut() {
                open.dir = Some(direction);
            }
            return Ok(());
        }
        if let Some(rest) = keyword_rest(statement, "classDef") {
            return self.class_def(rest);
        }
        if let Some(rest) = keyword_rest(statement, "class") {
            return self.class_assignment(rest);
        }
        if let Some(rest) = keyword_rest(statement, "style") {
            return self.style(rest);
        }
        if let Some(rest) = keyword_rest(statement, "linkStyle") {
            return self.link_style(rest);
        }
        self.chain(statement)
    }

    fn open_subgraph(&mut self, rest: &str) {
        let (id, title) = parse_subgraph_header(rest);
        self.open_subgraphs.push(OpenSubgraph {
            id,
            title,
            members: Vec::new(),
            dir: None,
        });
    }

    fn close_subgraph(&mut self) -> Result<(), DiagramError> {
        let open = self
            .open_subgraphs
            .pop()
            .ok_or_else(|| syntax("짝이 없는 end"))?;
        let id = open
            .id
            .unwrap_or_else(|| format!("subGraph{}", self.subgraph_count));
        self.subgraph_count += 1;
        let mut nodes: Vec<String> = Vec::new();
        for member in open.members {
            let is_taken = self
                .chart
                .subgraphs
                .iter()
                .any(|subgraph| subgraph.nodes.contains(&member));
            if !nodes.contains(&member) && !is_taken {
                nodes.push(member);
            }
        }
        self.chart.subgraphs.push(Subgraph {
            id: id.clone(),
            title: open.title,
            nodes,
            dir: open.dir,
        });
        self.add_member(&id);
        Ok(())
    }

    fn add_member(&mut self, id: &str) {
        if let Some(open) = self.open_subgraphs.last_mut() {
            open.members.push(id.to_string());
        }
    }

    fn class_def(&mut self, rest: &str) -> Result<(), DiagramError> {
        let (names, styles) = rest
            .split_once(char::is_whitespace)
            .ok_or_else(|| syntax("classDef 형식"))?;
        let styles = split_styles(styles);
        for name in names.split(',') {
            self.chart
                .class_styles
                .insert(name.trim().to_string(), styles.clone());
        }
        Ok(())
    }

    fn class_assignment(&mut self, rest: &str) -> Result<(), DiagramError> {
        let (ids, class_name) = rest
            .rsplit_once(char::is_whitespace)
            .ok_or_else(|| syntax("class 형식"))?;
        for id in ids.split(',') {
            self.ensure_vertex(id.trim(), None)
                .classes
                .push(class_name.trim().to_string());
        }
        Ok(())
    }

    fn style(&mut self, rest: &str) -> Result<(), DiagramError> {
        let (id, styles) = rest
            .split_once(char::is_whitespace)
            .ok_or_else(|| syntax("style 형식"))?;
        let styles = split_styles(styles);
        self.ensure_vertex(id.trim(), None).styles.extend(styles);
        Ok(())
    }

    fn link_style(&mut self, rest: &str) -> Result<(), DiagramError> {
        let (targets, styles) = rest
            .split_once(char::is_whitespace)
            .ok_or_else(|| syntax("linkStyle 형식"))?;
        let styles = split_styles(styles);
        if targets == "default" {
            self.default_link_style = styles;
            return Ok(());
        }
        for target in targets.split(',') {
            let index: usize = target
                .trim()
                .parse()
                .map_err(|_| syntax("linkStyle 번호"))?;
            let edge = self
                .chart
                .edges
                .get_mut(index)
                .ok_or_else(|| syntax("없는 링크 번호"))?;
            edge.styles = styles.clone();
        }
        Ok(())
    }

    fn ensure_vertex(&mut self, id: &str, node: Option<&ParsedNode>) -> &mut Vertex {
        let index = match self.vertex_index.get(id) {
            Some(index) => *index,
            None => {
                self.chart.vertices.push(Vertex {
                    id: id.to_string(),
                    text: id.to_string(),
                    shape: NodeShape::Rect,
                    classes: Vec::new(),
                    styles: Vec::new(),
                });
                self.vertex_index
                    .insert(id.to_string(), self.chart.vertices.len() - 1);
                self.chart.vertices.len() - 1
            }
        };
        let vertex = &mut self.chart.vertices[index];
        if let Some(node) = node {
            if let Some((shape, text)) = &node.shape {
                vertex.shape = *shape;
                vertex.text = text.clone();
            }
            vertex.classes.extend(node.class_name.iter().cloned());
        }
        vertex
    }

    fn chain(&mut self, statement: &str) -> Result<(), DiagramError> {
        let mut cursor = Cursor {
            source: statement,
            position: 0,
        };
        let mut previous = self.node_group(&mut cursor)?;
        loop {
            cursor.skip_whitespace();
            if cursor.is_done() {
                return Ok(());
            }
            let link = scan_link(&mut cursor)?;
            let next = self.node_group(&mut cursor)?;
            for start in &previous {
                for end in &next {
                    self.chart.edges.push(Edge {
                        start: start.clone(),
                        end: end.clone(),
                        text: link.text.clone(),
                        arrow_start: link.arrow_start,
                        arrow_end: link.arrow_end,
                        stroke: link.stroke,
                        length: link.length,
                        styles: Vec::new(),
                    });
                }
            }
            previous = next;
        }
    }

    fn node_group(&mut self, cursor: &mut Cursor) -> Result<Vec<String>, DiagramError> {
        let mut ids = Vec::new();
        loop {
            cursor.skip_whitespace();
            let node = scan_node(cursor)?;
            self.ensure_vertex(&node.id, Some(&node));
            self.add_member(&node.id);
            ids.push(node.id);
            cursor.skip_whitespace();
            if !cursor.eat("&") {
                return Ok(ids);
            }
        }
    }
}

fn parse_subgraph_header(rest: &str) -> (Option<String>, String) {
    let rest = rest.trim();
    if let Some(open) = rest.find('[') {
        let id = rest[..open].trim().to_string();
        let title = rest[open + 1..].trim_end_matches(']');
        return (Some(id), unquote(title.trim()).to_string());
    }
    let title = unquote(rest).to_string();
    // FlowDB: id와 제목이 같고 공백이 있으면 id 없이 subGraphN을 쓴다
    if rest.contains(char::is_whitespace) || rest.starts_with('"') {
        (None, title)
    } else {
        (Some(rest.to_string()), title)
    }
}

fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(text)
}

fn split_styles(styles: &str) -> Vec<String> {
    styles
        .trim()
        .trim_end_matches(';')
        .split(',')
        .map(|style| style.trim().to_string())
        .filter(|style| !style.is_empty())
        .collect()
}

struct Cursor<'a> {
    source: &'a str,
    position: usize,
}

impl Cursor<'_> {
    fn rest(&self) -> &str {
        &self.source[self.position..]
    }

    fn is_done(&self) -> bool {
        self.position >= self.source.len()
    }

    fn skip_whitespace(&mut self) {
        let rest = self.rest();
        self.position += rest.len() - rest.trim_start().len();
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.rest().starts_with(token) {
            self.position += token.len();
            return true;
        }
        false
    }
}

struct ParsedNode {
    id: String,
    shape: Option<(NodeShape, String)>,
    class_name: Option<String>,
}

fn is_id_character(rest: &str, character: char) -> bool {
    if character.is_alphanumeric()
        || matches!(
            character,
            '_' | '!' | '#' | '$' | '\'' | '*' | '+' | '?' | '\\' | '/' | '`'
        )
    {
        return true;
    }
    let next = rest[character.len_utf8()..].chars().next();
    match character {
        '-' => !matches!(next, Some('-' | '>' | '.')),
        '=' => next != Some('='),
        '.' => true,
        _ => false,
    }
}

fn scan_node(cursor: &mut Cursor) -> Result<ParsedNode, DiagramError> {
    let start = cursor.position;
    while let Some(character) = cursor.rest().chars().next() {
        if !is_id_character(cursor.rest(), character) {
            break;
        }
        cursor.position += character.len_utf8();
    }
    let id = cursor.source[start..cursor.position].to_string();
    if id.is_empty() {
        return Err(syntax(format!("노드가 필요합니다: {}", cursor.rest())));
    }
    let shape = scan_shape(cursor)?;
    let class_name = if cursor.eat(":::") {
        let class_start = cursor.position;
        while cursor.rest().starts_with(|character: char| {
            character.is_alphanumeric() || character == '_' || character == '-'
        }) {
            cursor.position += cursor.rest().chars().next().map_or(1, char::len_utf8);
        }
        Some(cursor.source[class_start..cursor.position].to_string())
    } else {
        None
    };
    Ok(ParsedNode {
        id,
        shape,
        class_name,
    })
}

const SHAPE_OPENERS: [(&str, NodeShape); 12] = [
    ("(((", NodeShape::DoubleCircle),
    ("((", NodeShape::Circle),
    ("([", NodeShape::Stadium),
    ("[[", NodeShape::Subroutine),
    ("[(", NodeShape::Cylinder),
    ("[/", NodeShape::LeanRight),
    ("[\\", NodeShape::LeanLeft),
    ("{{", NodeShape::Hexagon),
    ("(", NodeShape::Rounded),
    ("[", NodeShape::Rect),
    ("{", NodeShape::Diamond),
    (">", NodeShape::Odd),
];

fn closers(shape: NodeShape) -> &'static [(&'static str, NodeShape)] {
    match shape {
        NodeShape::DoubleCircle => &[(")))", NodeShape::DoubleCircle)],
        NodeShape::Circle => &[("))", NodeShape::Circle)],
        NodeShape::Stadium => &[("])", NodeShape::Stadium)],
        NodeShape::Subroutine => &[("]]", NodeShape::Subroutine)],
        NodeShape::Cylinder => &[(")]", NodeShape::Cylinder)],
        NodeShape::LeanRight => &[("/]", NodeShape::LeanRight), ("\\]", NodeShape::Trapezoid)],
        NodeShape::LeanLeft => &[
            ("\\]", NodeShape::LeanLeft),
            ("/]", NodeShape::InvTrapezoid),
        ],
        NodeShape::Hexagon => &[("}}", NodeShape::Hexagon)],
        NodeShape::Rounded => &[(")", NodeShape::Rounded)],
        NodeShape::Diamond => &[("}", NodeShape::Diamond)],
        NodeShape::Odd => &[("]", NodeShape::Odd)],
        _ => &[("]", NodeShape::Rect)],
    }
}

fn scan_shape(cursor: &mut Cursor) -> Result<Option<(NodeShape, String)>, DiagramError> {
    let Some((opener, shape)) = SHAPE_OPENERS
        .iter()
        .find(|(opener, _)| cursor.rest().starts_with(opener))
    else {
        return Ok(None);
    };
    cursor.position += opener.len();
    let rest = cursor.rest();
    if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted
            .find('"')
            .ok_or_else(|| syntax("닫는 따옴표가 없습니다"))?;
        let text = quoted[..end].to_string();
        cursor.position += end + 2;
        let (closer, final_shape) = closers(*shape)
            .iter()
            .find(|(closer, _)| cursor.rest().starts_with(closer))
            .ok_or_else(|| syntax("노드 모양이 닫히지 않았습니다"))?;
        cursor.position += closer.len();
        return Ok(Some((*final_shape, text)));
    }
    let (end, closer, final_shape) = closers(*shape)
        .iter()
        .filter_map(|(closer, final_shape)| {
            rest.find(closer).map(|end| (end, *closer, *final_shape))
        })
        .min_by_key(|(end, _, _)| *end)
        .ok_or_else(|| syntax("노드 모양이 닫히지 않았습니다"))?;
    let text = rest[..end].trim().to_string();
    cursor.position += end + closer.len();
    Ok(Some((final_shape, text)))
}

struct LinkToken {
    arrow_start: ArrowType,
    arrow_end: ArrowType,
    stroke: Stroke,
    length: i32,
    text: String,
}

fn arrow_of(character: Option<char>) -> ArrowType {
    match character {
        Some('>' | '<') => ArrowType::Point,
        Some('o') => ArrowType::Circle,
        Some('x') => ArrowType::Cross,
        _ => ArrowType::None,
    }
}

/// flow.jison LINK 규칙: `[xo<]?--+[-xo>]`, `[xo<]?==+[=xo>]`, `[xo<]?-?.+-[xo>]?`, `~~~+`
fn match_full_link(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'x' | b'o' | b'<')));
    let body_start = index;
    if rest[index..].starts_with("~~~") {
        while bytes.get(index) == Some(&b'~') {
            index += 1;
        }
        return Some(index);
    }
    for fill in *b"-=" {
        let mut end = body_start;
        while bytes.get(end) == Some(&fill) {
            end += 1;
        }
        if end - body_start < 2 {
            continue;
        }
        let terminator =
            |byte: Option<&u8>| matches!(byte, Some(b'x' | b'o' | b'>')) || byte == Some(&fill);
        if terminator(bytes.get(end)) {
            return Some(end + 1);
        }
        if end - body_start >= 3 {
            return Some(end);
        }
    }
    let mut end = body_start + usize::from(bytes.get(body_start) == Some(&b'-'));
    let dots_start = end;
    while bytes.get(end) == Some(&b'.') {
        end += 1;
    }
    if end > dots_start && bytes.get(end) == Some(&b'-') {
        end += 1;
        if matches!(bytes.get(end), Some(b'x' | b'o' | b'>')) {
            end += 1;
        }
        return Some(end);
    }
    None
}

/// 선 부분(화살표 글자를 뺀 `--`, `==`, `-.-` 등)에서 선 모양과 길이를 읽는다
fn line_stroke(line: &str) -> (Stroke, i32) {
    let dots = line.chars().filter(|character| *character == '.').count();
    if dots > 0 {
        return (Stroke::Dotted, i32::try_from(dots).unwrap_or(1));
    }
    let length = i32::try_from(line.chars().count()).unwrap_or(1) - 1;
    let stroke = match line.chars().next() {
        Some('=') => Stroke::Thick,
        Some('~') => Stroke::Invisible,
        _ => Stroke::Normal,
    };
    (stroke, length)
}

/// FlowDB.destructLink: 끝 토큰과 시작 토큰에서 화살표·선 모양·길이를 읽는다
fn destruct_link(end_token: &str, start_token: Option<&str>) -> Result<LinkToken, DiagramError> {
    let token = end_token.trim();
    let last = token.chars().last();
    let mut line = &token[..token.len() - last.map_or(0, char::len_utf8)];
    let arrow_end = arrow_of(last.filter(|character| matches!(character, 'x' | '>' | 'o')));
    let mut arrow_start = ArrowType::None;
    let first = token.chars().next();
    let is_double = matches!(
        (first, last),
        (Some('<'), Some('>')) | (Some('x'), Some('x')) | (Some('o'), Some('o'))
    );
    if arrow_end != ArrowType::None && start_token.is_none() && is_double {
        arrow_start = arrow_end;
        line = &line[1..];
    }
    let (stroke, length) = line_stroke(line);
    let start_arrow = start_token
        .and_then(|start| start.trim().chars().next())
        .filter(|character| matches!(character, '<' | 'x' | 'o'));
    if start_arrow.is_some() {
        arrow_start = arrow_of(start_arrow);
    }
    Ok(LinkToken {
        arrow_start,
        arrow_end,
        stroke,
        length: length.max(1),
        text: String::new(),
    })
}

fn scan_link(cursor: &mut Cursor) -> Result<LinkToken, DiagramError> {
    cursor.skip_whitespace();
    let rest = cursor.rest();
    if let Some(length) = match_full_link(rest) {
        let token = &rest[..length];
        let mut link = destruct_link(token, None)?;
        cursor.position += length;
        cursor.skip_whitespace();
        if cursor.eat("|") {
            let end = cursor
                .rest()
                .find('|')
                .ok_or_else(|| syntax("링크 라벨이 닫히지 않았습니다"))?;
            link.text = unquote(cursor.rest()[..end].trim()).to_string();
            cursor.position += end + 1;
        }
        return Ok(link);
    }
    scan_text_link(cursor)
}

/// `-- 글자 -->`, `== 글자 ==>`, `-. 글자 .->`
fn scan_text_link(cursor: &mut Cursor) -> Result<LinkToken, DiagramError> {
    let rest = cursor.rest();
    let start_length = usize::from(matches!(rest.as_bytes().first(), Some(b'x' | b'o' | b'<')));
    let opener = &rest[start_length..];
    if !(opener.starts_with("--") || opener.starts_with("==") || opener.starts_with("-.")) {
        return Err(syntax(format!("링크가 필요합니다: {rest}")));
    }
    let start_token = &rest[..start_length + 2];
    let body = &rest[start_token.len()..];
    let (text_end, end_length) =
        find_link_end(body).ok_or_else(|| syntax("링크 라벨이 닫히지 않았습니다"))?;
    let end_token = body[text_end..text_end + end_length].trim().to_string();
    let mut link = destruct_link(&end_token, Some(start_token))?;
    link.text = unquote(body[..text_end].trim()).to_string();
    cursor.position += start_token.len() + text_end + end_length;
    Ok(link)
}

fn find_link_end(body: &str) -> Option<(usize, usize)> {
    body.char_indices().find_map(|(index, _)| {
        let candidate = &body[index..];
        let leading = candidate.len() - candidate.trim_start().len();
        if leading == 0 && index > 0 && !body[..index].ends_with(char::is_whitespace) {
            return None;
        }
        match_full_link(candidate.trim_start()).map(|length| (index, leading + length))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기본_연결과_방향을_읽는다() {
        let chart = parse("graph TD; A-->B").expect("parse");
        assert_eq!(chart.direction, Direction::TopBottom);
        assert_eq!(chart.vertices.len(), 2);
        assert_eq!(chart.edges[0].arrow_end, ArrowType::Point);
        assert_eq!(chart.edges[0].length, 1);
    }

    #[test]
    fn 모양과_따옴표_라벨을_읽는다() {
        let chart =
            parse("flowchart LR\n  A[\"local·CI<br/>검증\"] --> B{Valid?}\n  B --> C[/Trap\\]")
                .expect("parse");
        assert_eq!(chart.vertices[0].text, "local·CI<br/>검증");
        assert_eq!(chart.vertices[1].shape, NodeShape::Diamond);
        assert_eq!(chart.vertices[2].shape, NodeShape::Trapezoid);
    }

    #[test]
    fn 링크_라벨과_선_모양을_읽는다() {
        let chart = parse(
            "flowchart TD\n A -->|Yes| B\n A -- No --> C\n B -.-> D\n B ==> E\n C --- E\n A --o F",
        )
        .expect("parse");
        let texts: Vec<&str> = chart.edges.iter().map(|edge| edge.text.as_str()).collect();
        assert_eq!(texts, ["Yes", "No", "", "", "", ""]);
        assert_eq!(chart.edges[2].stroke, Stroke::Dotted);
        assert_eq!(chart.edges[3].stroke, Stroke::Thick);
        assert_eq!(chart.edges[4].arrow_end, ArrowType::None);
        assert_eq!(chart.edges[5].arrow_end, ArrowType::Circle);
    }

    #[test]
    fn 안쪽_subgraph가_먼저_닫히고_노드는_안쪽에_속한다() {
        let chart = parse(
            "flowchart TD\nsubgraph outer [Outer]\n  subgraph inner\n    A --> B\n  end\n  C\nend",
        )
        .expect("parse");
        assert_eq!(chart.subgraphs[0].id, "inner");
        assert_eq!(chart.subgraphs[0].nodes, ["A", "B"]);
        assert_eq!(chart.subgraphs[1].nodes, ["inner", "C"]);
        assert_eq!(chart.subgraphs[1].title, "Outer");
    }

    #[test]
    fn class와_style을_모은다() {
        let chart = parse("flowchart LR\nclassDef warn fill:#fde68a,stroke:#b45309\nA:::warn --> B\nclass B warn\nstyle A stroke-width:2px").expect("parse");
        assert_eq!(chart.vertices[0].classes, ["warn"]);
        assert_eq!(chart.vertices[1].classes, ["warn"]);
        assert_eq!(chart.vertices[0].styles, ["stroke-width:2px"]);
        assert_eq!(chart.class_styles["warn"].len(), 2);
    }
}
