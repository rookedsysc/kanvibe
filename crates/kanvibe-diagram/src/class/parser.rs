//! classDiagram 정의 파서. classDiagram.jison 문장의 일부와 ClassDB의 저장 순서를 따른다.
//!
//! 지원: `class 이름[~T~]["라벨"][:::스타일] { 멤버 }`, `이름 : 멤버`, `<<주석>> 이름`, 관계 여덟 가지(`<|`·`*`·`o`·
//! `<`·`()` 양 끝 × `--`·`..`)와 양 끝 다중성 문자열·`: 라벨`, `note for 이름 "글"`·`note "글"`, `direction`,
//! `namespace 이름 { … }`(중첩·점 이름), `style`·`classDef`·`cssClass`, `%%` 주석.
//! 화면에 영향이 없는 `click`·`link`·`callback`·`accTitle`·`accDescr`는 건너뛴다.

use super::member::{ClassMember, attribute_member, is_method, method_member};
use crate::DiagramError;
use crate::flowchart::parser::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationKind {
    Aggregation,
    Extension,
    Composition,
    Dependency,
    Lollipop,
}

#[derive(Clone, Debug, Default)]
pub struct ClassNode {
    pub id: String,
    pub generic: String,
    pub label: String,
    pub annotations: Vec<String>,
    pub members: Vec<ClassMember>,
    pub methods: Vec<ClassMember>,
    /// cssClasses: `default` 뒤에 `:::`·cssClass로 붙인 이름을 공백으로 잇는다
    pub css_classes: String,
    /// style·classDef가 넣은 `속성:값` 선언
    pub styles: Vec<String>,
    /// 감싸는 namespace의 id
    pub parent: Option<String>,
}

impl ClassNode {
    /// 제목: 라벨 뒤에 제네릭 `<T>`
    pub fn title(&self) -> String {
        if self.generic.is_empty() {
            self.label.clone()
        } else {
            format!("{}<{}>", self.label, self.generic)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Relation {
    pub start: String,
    pub end: String,
    pub start_kind: Option<RelationKind>,
    pub end_kind: Option<RelationKind>,
    pub is_dashed: bool,
    pub title: String,
    /// 시작 쪽·끝 쪽 다중성(관계 앞뒤의 따옴표 문자열)
    pub start_label: String,
    pub end_label: String,
}

#[derive(Clone, Debug)]
pub struct Note {
    pub text: String,
    pub class_id: Option<String>,
    pub parent: Option<String>,
}

/// namespace 묶음. 점으로 이은 이름은 조상 묶음을 함께 만든다
#[derive(Clone, Debug)]
pub struct Namespace {
    pub id: String,
    pub label: String,
    pub parent: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ClassDiagram {
    pub direction: Direction,
    pub classes: Vec<ClassNode>,
    pub relations: Vec<Relation>,
    pub notes: Vec<Note>,
    /// 막대사탕 관계가 만든 인터페이스 노드의 라벨
    pub interfaces: Vec<String>,
    pub namespaces: Vec<Namespace>,
}

const SKIPPED_KEYWORDS: [&str; 5] = ["click ", "link ", "callback ", "accTitle", "accDescr"];

/// 줄을 읽는 동안의 상태: 열린 클래스 본문과 열린 namespace(바깥부터)
#[derive(Default)]
struct ParseState {
    open_class: Option<String>,
    namespaces: Vec<String>,
}

pub fn parse(definition: &str) -> Result<ClassDiagram, DiagramError> {
    let body = crate::directive::diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    if !matches!(header, "classDiagram" | "classDiagram-v2") {
        return Err(DiagramError::Syntax(format!(
            "classDiagram 머리줄: {header}"
        )));
    }
    let mut diagram = ClassDiagram {
        direction: Direction::TopBottom,
        classes: Vec::new(),
        relations: Vec::new(),
        notes: Vec::new(),
        interfaces: Vec::new(),
        namespaces: Vec::new(),
    };
    let mut state = ParseState::default();
    for line in lines {
        if let Some(class_id) = &state.open_class {
            if read_body_line(&mut diagram, class_id, line) {
                state.open_class = None;
            }
            continue;
        }
        parse_statement(&mut diagram, &mut state, line)?;
    }
    Ok(diagram)
}

/// 중괄호 안 한 줄. `}`를 만나면 참을 돌려 본문을 닫는다
fn read_body_line(diagram: &mut ClassDiagram, class_id: &str, line: &str) -> bool {
    let (member, is_closed) = match line.split_once('}') {
        Some((member, _)) => (member, true),
        None => (line, false),
    };
    diagram.add_member(class_id, member);
    is_closed
}

/// 문장 하나를 읽는다. 클래스 본문이나 namespace가 열리고 닫히면 `state`에 남긴다
fn parse_statement(
    diagram: &mut ClassDiagram,
    state: &mut ParseState,
    line: &str,
) -> Result<(), DiagramError> {
    let namespace = state.namespaces.last().cloned();
    if let Some(value) = line.strip_prefix("direction") {
        diagram.direction = Direction::parse(value.trim()).unwrap_or(diagram.direction);
    } else if let Some(rest) = line.strip_prefix("class ") {
        state.open_class = diagram.parse_class_statement(rest.trim(), namespace);
    } else if line.starts_with("<<") {
        diagram.parse_annotation_statement(line)?;
    } else if let Some(rest) = line.strip_prefix("note") {
        diagram.parse_note(rest.trim(), namespace)?;
    } else if let Some(rest) = line.strip_prefix("namespace ") {
        let id = diagram.add_namespace(rest.trim_end_matches('{').trim(), &state.namespaces);
        state.namespaces.push(id);
    } else if line.starts_with('}') {
        state.namespaces.pop();
    } else if !diagram.parse_style_statement(line) && !is_skipped(line) {
        diagram.parse_relation_or_member(line)?;
    }
    Ok(())
}

fn is_skipped(line: &str) -> bool {
    line == "{"
        || SKIPPED_KEYWORDS
            .iter()
            .any(|keyword| line.starts_with(keyword))
}

impl ClassDiagram {
    /// addClass: `이름~T~`를 이름과 제네릭으로 나누고, 처음 나온 순서대로 넣는다
    fn add_class(&mut self, raw_id: &str) -> String {
        let (name, generic) = split_class_name(raw_id);
        if !self.classes.iter().any(|class| class.id == name) {
            self.classes.push(ClassNode {
                id: name.clone(),
                generic,
                label: name.clone(),
                css_classes: "default".to_string(),
                ..ClassNode::default()
            });
        }
        name
    }

    pub(super) fn class_mut(&mut self, id: &str) -> Option<&mut ClassNode> {
        self.classes.iter_mut().find(|class| class.id == id)
    }

    /// addMember: `<<…>>`는 주석, `)`가 있으면 메서드, 아니면 속성
    fn add_member(&mut self, raw_id: &str, member: &str) {
        let id = self.add_class(raw_id);
        let member = member.trim();
        let Some(class) = self.class_mut(&id) else {
            return;
        };
        if let Some(annotation) = member
            .strip_prefix("<<")
            .and_then(|rest| rest.strip_suffix(">>"))
        {
            class.annotations.push(annotation.to_string());
        } else if is_method(member) {
            class.methods.push(method_member(member));
        } else if !member.is_empty() {
            class.members.push(attribute_member(member));
        }
    }

    /// `이름[~T~] ["라벨"] [:::스타일] [{ 멤버 …]`. namespace 안이면 그 묶음에 넣는다
    fn parse_class_statement(&mut self, rest: &str, namespace: Option<String>) -> Option<String> {
        let name_end = rest
            .find(|character: char| character.is_whitespace() || "[{:".contains(character))
            .unwrap_or(rest.len());
        let id = self.add_class(&rest[..name_end]);
        let mut tail = rest[name_end..].trim_start();
        if let Some(label) = tail
            .strip_prefix("[\"")
            .and_then(|inner| inner.split_once("\"]"))
        {
            if let Some(class) = self.class_mut(&id) {
                class.label = label.0.to_string();
            }
            tail = label.1.trim_start();
        }
        if let Some(style_tail) = tail.strip_prefix(":::") {
            let name_end = style_tail.find(['{', ' ']).unwrap_or(style_tail.len());
            self.set_css_class(&id, style_tail[..name_end].trim());
            tail = style_tail[name_end..].trim_start();
        }
        if namespace.is_some()
            && let Some(class) = self.class_mut(&id)
        {
            class.parent = namespace;
        }
        let (_, body) = tail.split_once('{')?;
        if read_body_line(self, &id, body) {
            return None;
        }
        Some(id)
    }

    /// `<<주석>> 이름`. 원본처럼 없는 클래스면 오류다
    fn parse_annotation_statement(&mut self, line: &str) -> Result<(), DiagramError> {
        let (annotation, name) = line[2..]
            .split_once(">>")
            .ok_or_else(|| DiagramError::Syntax(format!("주석 문장: {line}")))?;
        let (id, _) = split_class_name(name.trim());
        let class = self
            .class_mut(&id)
            .ok_or_else(|| DiagramError::Syntax(format!("없는 클래스: {id}")))?;
        class.annotations.push(annotation.trim().to_string());
        Ok(())
    }

    /// `note for 이름 "글"` 또는 `note "글"`
    fn parse_note(&mut self, rest: &str, namespace: Option<String>) -> Result<(), DiagramError> {
        let (target, quoted) = match rest.strip_prefix("for ") {
            Some(after) => {
                let (name, quoted) = after
                    .trim()
                    .split_once(char::is_whitespace)
                    .unwrap_or((after, ""));
                (Some(name.to_string()), quoted.trim())
            }
            None => (None, rest),
        };
        let text =
            unquote(quoted).ok_or_else(|| DiagramError::Syntax(format!("note 문장: {rest}")))?;
        self.notes.push(Note {
            text: text.to_string(),
            class_id: target,
            parent: namespace,
        });
        Ok(())
    }

    /// 관계(`A "1" *-- "n" B : 라벨`), 멤버(`A : +x`), 또는 이름만 쓴 클래스
    fn parse_relation_or_member(&mut self, line: &str) -> Result<(), DiagramError> {
        let (head, label) = match find_outside_quotes(line, ":") {
            Some(index) => (&line[..index], Some(line[index + 1..].trim())),
            None => (line, None),
        };
        if let Some(operator) = find_line_operator(head) {
            let relation = parse_relation(head, operator, label.unwrap_or(""))?;
            self.add_relation(relation);
            return Ok(());
        }
        match label {
            Some(member) => self.add_member(head.trim(), member),
            None if is_class_name(head) => {
                self.add_class(head);
            }
            None => return Err(DiagramError::Syntax(format!("classDiagram 문장: {line}"))),
        }
        Ok(())
    }

    /// addRelation: 막대사탕 한쪽 관계는 그 끝을 인터페이스 노드로 바꾼다
    fn add_relation(&mut self, mut relation: Relation) {
        let is_lollipop = |kind: Option<RelationKind>| kind == Some(RelationKind::Lollipop);
        let is_plain = |kind: Option<RelationKind>| kind.is_none();
        if is_lollipop(relation.start_kind) && is_plain(relation.end_kind) {
            relation.end = self.add_class(&relation.end);
            relation.start = self.add_interface(&relation.start);
        } else if is_lollipop(relation.end_kind) && is_plain(relation.start_kind) {
            relation.start = self.add_class(&relation.start);
            relation.end = self.add_interface(&relation.end);
        } else {
            relation.start = self.add_class(&relation.start);
            relation.end = self.add_class(&relation.end);
        }
        self.relations.push(relation);
    }

    fn add_interface(&mut self, label: &str) -> String {
        self.interfaces.push(label.to_string());
        format!("interface{}", self.interfaces.len() - 1)
    }
}

/// `Pond~T~` → (`Pond`, `T`). 백틱 이름은 백틱을 뗀다
fn split_class_name(raw_id: &str) -> (String, String) {
    let id = raw_id.trim().trim_matches('`');
    match id.find('~').filter(|index| *index > 0) {
        Some(index) => {
            let generic = id[index + 1..].split('~').next().unwrap_or("");
            (id[..index].to_string(), generic.to_string())
        }
        None => (id.to_string(), String::new()),
    }
}

fn is_class_name(text: &str) -> bool {
    let (name, _) = split_class_name(text);
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
}

fn unquote(text: &str) -> Option<&str> {
    text.strip_prefix('"')?.strip_suffix('"')
}

/// 따옴표 밖에서 처음 나오는 `needle`의 위치
fn find_outside_quotes(text: &str, needle: &str) -> Option<usize> {
    let mut is_quoted = false;
    for (index, character) in text.char_indices() {
        if character == '"' {
            is_quoted = !is_quoted;
        } else if !is_quoted && text[index..].starts_with(needle) {
            return Some(index);
        }
    }
    None
}

/// 선 토큰(`--` 실선, `..` 점선)의 위치
fn find_line_operator(head: &str) -> Option<usize> {
    let solid = find_outside_quotes(head, "--");
    let dotted = find_outside_quotes(head, "..");
    match (solid, dotted) {
        (Some(solid), Some(dotted)) => Some(solid.min(dotted)),
        (solid, dotted) => solid.or(dotted),
    }
}

fn parse_relation(head: &str, operator: usize, title: &str) -> Result<Relation, DiagramError> {
    let is_dashed = head[operator..].starts_with("..");
    let (left, start_kind) = strip_left_kind(head[..operator].trim_end());
    let (right, end_kind) = strip_right_kind(head[operator + 2..].trim_start());
    let (start_label, start) = split_trailing_quote(left.trim_end());
    let (end_label, end) = split_leading_quote(right.trim_start());
    if start.is_empty() || end.is_empty() {
        return Err(DiagramError::Syntax(format!("관계 문장: {head}")));
    }
    Ok(Relation {
        start: start.to_string(),
        end: end.to_string(),
        start_kind,
        end_kind,
        is_dashed,
        title: title.to_string(),
        start_label: start_label.to_string(),
        end_label: end_label.to_string(),
    })
}

/// 선 왼쪽 끝의 관계 표시(`<|`·`()`·`*`·`<`·` o`)
fn strip_left_kind(left: &str) -> (&str, Option<RelationKind>) {
    let marks = [
        ("<|", RelationKind::Extension),
        ("()", RelationKind::Lollipop),
        ("*", RelationKind::Composition),
        ("<", RelationKind::Dependency),
    ];
    for (mark, kind) in marks {
        if let Some(rest) = left.strip_suffix(mark) {
            return (rest, Some(kind));
        }
    }
    match left.strip_suffix('o') {
        Some(rest) if rest.is_empty() || rest.ends_with([' ', '"']) => {
            (rest, Some(RelationKind::Aggregation))
        }
        _ => (left, None),
    }
}

/// 선 오른쪽 끝의 관계 표시(`|>`·`()`·`*`·`>`·`o `)
fn strip_right_kind(right: &str) -> (&str, Option<RelationKind>) {
    let marks = [
        ("|>", RelationKind::Extension),
        ("()", RelationKind::Lollipop),
        ("*", RelationKind::Composition),
        (">", RelationKind::Dependency),
    ];
    for (mark, kind) in marks {
        if let Some(rest) = right.strip_prefix(mark) {
            return (rest, Some(kind));
        }
    }
    match right.strip_prefix('o') {
        Some(rest) if rest.is_empty() || rest.starts_with([' ', '"']) => {
            (rest, Some(RelationKind::Aggregation))
        }
        _ => (right, None),
    }
}

/// `이름 "1"` → (`1`, `이름`)
fn split_trailing_quote(left: &str) -> (&str, &str) {
    let Some(without_close) = left.strip_suffix('"') else {
        return ("", left.trim());
    };
    match without_close.rfind('"') {
        Some(open) => (&without_close[open + 1..], without_close[..open].trim()),
        None => ("", left.trim()),
    }
}

/// `"many" 이름` → (`many`, `이름`)
fn split_leading_quote(right: &str) -> (&str, &str) {
    let Some(without_open) = right.strip_prefix('"') else {
        return ("", right.trim());
    };
    match without_open.split_once('"') {
        Some((label, rest)) => (label, rest.trim()),
        None => ("", right.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relation_of(line: &str) -> Relation {
        let diagram = parse(&format!("classDiagram\n{line}")).expect("parse");
        diagram.relations[0].clone()
    }

    #[test]
    fn 관계_여덟_가지를_읽는다() {
        let cases = [
            ("A <|-- B", Some(RelationKind::Extension), None, false),
            ("A *-- B", Some(RelationKind::Composition), None, false),
            ("A o-- B", Some(RelationKind::Aggregation), None, false),
            ("A --> B", None, Some(RelationKind::Dependency), false),
            ("A ..> B", None, Some(RelationKind::Dependency), true),
            ("A ..|> B", None, Some(RelationKind::Extension), true),
            ("A -- B", None, None, false),
            ("A .. B", None, None, true),
        ];
        for (line, start_kind, end_kind, is_dashed) in cases {
            let relation = relation_of(line);
            assert_eq!(
                (relation.start_kind, relation.end_kind, relation.is_dashed),
                (start_kind, end_kind, is_dashed),
                "{line}"
            );
            assert_eq!((relation.start.as_str(), relation.end.as_str()), ("A", "B"));
        }
    }

    #[test]
    fn 다중성과_라벨() {
        let relation = relation_of("Pond \"1\" *-- \"many\" Duck : contains");
        assert_eq!(relation.start_label, "1");
        assert_eq!(relation.end_label, "many");
        assert_eq!(relation.title, "contains");
        assert_eq!(relation.start_kind, Some(RelationKind::Composition));
    }

    #[test]
    fn 클래스_본문과_멤버_문장() {
        let diagram = parse(
            "classDiagram\n class Pond~T~ {\n <<interface>>\n +List~T~ items\n +add(T item) bool\n }\n Pond : -int size",
        )
        .expect("parse");
        let pond = &diagram.classes[0];
        assert_eq!(pond.title(), "Pond<T>");
        assert_eq!(pond.annotations, vec!["interface"]);
        let texts = |members: &[ClassMember]| -> Vec<String> {
            members.iter().map(|member| member.text.clone()).collect()
        };
        assert_eq!(texts(&pond.members), vec!["+List<T> items", "-int size"]);
        assert_eq!(texts(&pond.methods), vec!["+add(T item) : bool"]);
    }

    #[test]
    fn 막대사탕은_인터페이스_노드를_만든다() {
        let diagram = parse("classDiagram\n Shape ()-- Circle").expect("parse");
        assert_eq!(diagram.interfaces, vec!["Shape"]);
        assert_eq!(diagram.relations[0].start, "interface0");
        assert_eq!(diagram.classes.len(), 1);
    }

    #[test]
    fn 노트와_방향() {
        let diagram =
            parse("classDiagram\n direction LR\n class A\n note for A \"memo\"\n note \"free\"")
                .expect("parse");
        assert_eq!(diagram.direction, Direction::LeftRight);
        assert_eq!(diagram.notes[0].class_id.as_deref(), Some("A"));
        assert_eq!(diagram.notes[1].text, "free");
    }
}
