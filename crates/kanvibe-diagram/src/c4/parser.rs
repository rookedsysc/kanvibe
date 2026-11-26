//! c4Diagram.jison과 c4Db.js의 요소·경계·관계 추가 규칙과 `UpdateElementStyle`·`UpdateRelStyle`·
//! `UpdateLayoutConfig`.
//!
//! 한 줄에 `키워드(인자, …)` 하나를 읽는다. 인자는 `"따옴표 글"`, 맨 글, `$키="값"`이다. 인자를 칸에 담는 규칙은
//! [`super::fields`]가 c4Db와 똑같이 따른다. 같은 별칭(관계는 같은 양 끝)을 다시 정의하면 앞의 칸을 그대로 두고
//! 새 인자로 덮어쓴다. 경계는 줄 끝 `{`로 열고 `}`로 닫는다. `$sprite`·`$tags`·`$link`는 그림을 그리지 않지만
//! 스프라이트가 있으면 모양·경계 위쪽에 48px 그림 자리가 생긴다.

use crate::DiagramError;

use super::fields::{Argument, FieldValue, Fields, parse_int};

/// 맨 바깥 경계 별칭(c4Db `boundaries[0]`)
pub(crate) const GLOBAL_BOUNDARY: &str = "global";
/// c4Db `c4ShapeInRow`·`c4BoundaryInRow` 기본값
const DEFAULT_SHAPES_IN_ROW: usize = 4;
const DEFAULT_BOUNDARIES_IN_ROW: usize = 2;

const PERSON_SLOTS: [&str; 6] = ["alias", "label", "descr", "sprite", "tags", "link"];
const CONTAINER_SLOTS: [&str; 7] = ["alias", "label", "techn", "descr", "sprite", "tags", "link"];
const RELATION_SLOTS: [&str; 8] = [
    "from", "to", "label", "techn", "descr", "sprite", "tags", "link",
];
const BOUNDARY_SLOTS: [&str; 5] = ["alias", "label", "type", "tags", "link"];
const DEPLOYMENT_SLOTS: [&str; 7] = ["alias", "label", "type", "descr", "sprite", "tags", "link"];
const ELEMENT_STYLE_SLOTS: [&str; 9] = [
    "bgColor",
    "fontColor",
    "borderColor",
    "shadowing",
    "shape",
    "sprite",
    "techn",
    "legendText",
    "legendSprite",
];
const RELATION_STYLE_SLOTS: [&str; 4] = ["textColor", "lineColor", "offsetX", "offsetY"];

/// `UpdateElementStyle`로 바꾼 색. 값은 CSS 색 글자 그대로다
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ShapeStyle {
    pub background: Option<String>,
    pub font: Option<String>,
    pub border: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct C4Element {
    /// `person`, `external_system_db` 같은 c4Db `typeC4Shape`
    pub kind: String,
    pub alias: String,
    pub label: String,
    pub techn: String,
    pub descr: String,
    pub parent: String,
    /// `$sprite`가 있으면 그림 자리(48px)를 비워 둔다
    pub has_sprite: bool,
    pub style: ShapeStyle,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct C4Boundary {
    pub alias: String,
    pub label: String,
    pub kind: String,
    pub descr: String,
    /// Deployment_Node·Node 계열이면 테두리가 실선이다
    pub is_deployment_node: bool,
    pub parent: String,
    pub has_sprite: bool,
    /// 경계 글자색(`fontColor`)은 글자에 `#444444`가 덮여 쓰이지 않는다
    pub style: ShapeStyle,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct C4Relation {
    /// `rel`, `birel`, `rel_u`, `rel_b` …
    pub kind: String,
    pub from: String,
    pub to: String,
    pub label: String,
    pub techn: String,
    pub text_color: Option<String>,
    pub line_color: Option<String>,
    pub offset_x: f64,
    pub offset_y: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct C4Diagram {
    pub diagram_type: String,
    pub title: String,
    pub elements: Vec<C4Element>,
    pub boundaries: Vec<C4Boundary>,
    pub relations: Vec<C4Relation>,
    pub shapes_in_row: usize,
    pub boundaries_in_row: usize,
}

/// 칸을 다 채우기 전의 요소·경계·관계
struct Record {
    kind: String,
    alias: String,
    parent: String,
    fields: Fields,
}

/// 관계는 양 끝으로 찾는다
struct RelationRecord {
    kind: String,
    from: String,
    to: String,
    fields: Fields,
}

struct ParseState {
    title: String,
    elements: Vec<Record>,
    /// `kind`는 배포 노드면 `node`, 아니면 빈 글
    boundaries: Vec<Record>,
    relations: Vec<RelationRecord>,
    parents: Vec<String>,
    shapes_in_row: usize,
    boundaries_in_row: usize,
}

pub(crate) fn parse(definition: &str) -> Result<C4Diagram, DiagramError> {
    let body = crate::directive::diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines.next().unwrap_or_default();
    let mut global = Fields::default();
    global.insert("label", FieldValue::Text(GLOBAL_BOUNDARY.to_string()));
    global.insert("type", FieldValue::Text(GLOBAL_BOUNDARY.to_string()));
    let mut state = ParseState {
        title: String::new(),
        elements: Vec::new(),
        boundaries: vec![Record {
            kind: String::new(),
            alias: GLOBAL_BOUNDARY.to_string(),
            parent: String::new(),
            fields: global,
        }],
        relations: Vec::new(),
        parents: vec![GLOBAL_BOUNDARY.to_string()],
        shapes_in_row: DEFAULT_SHAPES_IN_ROW,
        boundaries_in_row: DEFAULT_BOUNDARIES_IN_ROW,
    };
    for line in lines {
        parse_statement(&mut state, line)?;
    }
    let diagram_type = header.split_whitespace().next().unwrap_or_default();
    finish(state, diagram_type)
}

fn parse_statement(state: &mut ParseState, line: &str) -> Result<(), DiagramError> {
    if line == "}" {
        if state.parents.len() > 1 {
            state.parents.pop();
        }
        return Ok(());
    }
    if let Some(title) = line.strip_prefix("title ") {
        state.title = title.trim().to_string();
        return Ok(());
    }
    let Some(open) = line.find('(') else {
        return skip_or_reject(line);
    };
    let keyword = line[..open].trim();
    let close = line.rfind(')').unwrap_or(line.len());
    let arguments = split_arguments(&line[open + 1..close.max(open + 1)]);
    if let Some(kind) = element_kind(keyword) {
        add_element(state, kind, &arguments);
    } else if let Some(kind) = relation_kind(keyword) {
        add_relation(state, kind, keyword, arguments);
    } else {
        apply_other_statement(state, keyword, arguments);
    }
    Ok(())
}

/// 경계와 `Update*` 문장
fn apply_other_statement(state: &mut ParseState, keyword: &str, arguments: Vec<Argument>) {
    match keyword {
        "UpdateElementStyle" => update_element_style(state, &arguments),
        "UpdateRelStyle" => update_relation_style(state, &arguments),
        "UpdateLayoutConfig" => update_layout_config(state, &arguments),
        _ => add_boundary(state, keyword, arguments),
    }
}

/// accTitle·accDescr·direction 같은 줄은 그리지 않으므로 넘긴다
fn skip_or_reject(line: &str) -> Result<(), DiagramError> {
    let is_known = ["accTitle", "accDescr", "direction", "{"]
        .iter()
        .any(|prefix| line.starts_with(prefix));
    if is_known {
        Ok(())
    } else {
        Err(DiagramError::Syntax(format!("C4 문장 형식: {line}")))
    }
}

fn element_kind(keyword: &str) -> Option<&'static str> {
    let kind = match keyword {
        "Person" => "person",
        "Person_Ext" => "external_person",
        "System" => "system",
        "SystemDb" => "system_db",
        "SystemQueue" => "system_queue",
        "System_Ext" => "external_system",
        "SystemDb_Ext" => "external_system_db",
        "SystemQueue_Ext" => "external_system_queue",
        "Container" => "container",
        "ContainerDb" => "container_db",
        "ContainerQueue" => "container_queue",
        "Container_Ext" => "external_container",
        "ContainerDb_Ext" => "external_container_db",
        "ContainerQueue_Ext" => "external_container_queue",
        "Component" => "component",
        "ComponentDb" => "component_db",
        "ComponentQueue" => "component_queue",
        "Component_Ext" => "external_component",
        "ComponentDb_Ext" => "external_component_db",
        "ComponentQueue_Ext" => "external_component_queue",
        _ => return None,
    };
    Some(kind)
}

fn relation_kind(keyword: &str) -> Option<&'static str> {
    let kind = match keyword {
        "Rel" | "RelIndex" => "rel",
        "BiRel" => "birel",
        "Rel_Up" | "Rel_U" => "rel_u",
        "Rel_Down" | "Rel_D" => "rel_d",
        "Rel_Left" | "Rel_L" => "rel_l",
        "Rel_Right" | "Rel_R" => "rel_r",
        "Rel_Back" => "rel_b",
        _ => return None,
    };
    Some(kind)
}

/// 자리 인자 순서: 사람·시스템은 (별칭, 이름, 설명, …), 컨테이너·컴포넌트는 (별칭, 이름, 기술, 설명, …)
fn add_element(state: &mut ParseState, kind: &str, arguments: &[Argument]) {
    let has_techn = kind.contains("container") || kind.contains("component");
    let slots: &[&str] = if has_techn {
        &CONTAINER_SLOTS
    } else {
        &PERSON_SLOTS
    };
    let alias = arguments.first().map(argument_text).unwrap_or_default();
    let parent = state.parents.last().cloned().unwrap_or_default();
    let index = match state.elements.iter().position(|old| old.alias == alias) {
        Some(index) => index,
        None => {
            state.elements.push(Record {
                kind: String::new(),
                alias,
                parent: String::new(),
                fields: Fields::default(),
            });
            state.elements.len() - 1
        }
    };
    let record = &mut state.elements[index];
    record.kind = kind.to_string();
    record.parent = parent;
    record.fields.fill_slots(arguments, slots);
}

/// c4Db.addRel: 같은 (from, to) 관계는 칸을 덮어쓴다. 이름이 없으면 넘기고, RelIndex는 첫 인자(순번)를 버린다
fn add_relation(state: &mut ParseState, kind: &str, keyword: &str, mut arguments: Vec<Argument>) {
    if keyword == "RelIndex" && !arguments.is_empty() {
        arguments.remove(0);
    }
    if arguments.len() < 3 {
        return;
    }
    let from = argument_text(&arguments[0]);
    let to = argument_text(&arguments[1]);
    let existing = state
        .relations
        .iter()
        .position(|old| old.from == from && old.to == to);
    let index = existing.unwrap_or_else(|| {
        state.relations.push(RelationRecord {
            kind: String::new(),
            from,
            to,
            fields: Fields::default(),
        });
        state.relations.len() - 1
    });
    let record = &mut state.relations[index];
    record.kind = kind.to_string();
    record.fields.fill_slots(&arguments, &RELATION_SLOTS);
}

/// 경계: (별칭, 이름, 종류, …) 또는 배포 노드 (별칭, 이름, 종류, 설명, …). 고정 종류 경계는 문법이 셋째 자리에
/// 종류를 끼워 넣는다
fn add_boundary(state: &mut ParseState, keyword: &str, mut arguments: Vec<Argument>) {
    let (default_kind, fixed_kind) = match keyword {
        "Boundary" => ("system", None),
        "Enterprise_Boundary" => ("system", Some("ENTERPRISE")),
        "System_Boundary" => ("system", Some("SYSTEM")),
        "Container_Boundary" => ("container", Some("CONTAINER")),
        "Deployment_Node" | "Node" | "Node_L" | "Node_R" => ("node", None),
        _ => return,
    };
    if let Some(kind) = fixed_kind {
        let at = arguments.len().min(2);
        arguments.insert(at, Argument::Positional(kind.to_string()));
    }
    let is_deployment_node = default_kind == "node";
    let slots: &[&str] = if is_deployment_node {
        &DEPLOYMENT_SLOTS
    } else {
        &BOUNDARY_SLOTS
    };
    let alias = arguments.first().map(argument_text).unwrap_or_default();
    let parent = state.parents.last().cloned().unwrap_or_default();
    state.parents.push(alias.clone());
    let index = match state.boundaries.iter().position(|old| old.alias == alias) {
        Some(index) => index,
        None => {
            state.boundaries.push(Record {
                kind: String::new(),
                alias,
                parent: String::new(),
                fields: Fields::default(),
            });
            state.boundaries.len() - 1
        }
    };
    let record = &mut state.boundaries[index];
    record.kind = if is_deployment_node { "node" } else { "" }.to_string();
    record.parent = parent;
    record.fields.fill_slots(&arguments, slots);
    if arguments.len() < 3 {
        let kind = FieldValue::Text(default_kind.to_string());
        record.fields.insert("type", kind);
    }
}

/// c4Db.updateElStyle: 요소에서 먼저, 없으면 경계에서 별칭을 찾는다
fn update_element_style(state: &mut ParseState, arguments: &[Argument]) {
    let Some(alias) = arguments.first().map(argument_text) else {
        return;
    };
    let record = state
        .elements
        .iter_mut()
        .chain(state.boundaries.iter_mut())
        .find(|record| record.alias == alias);
    if let Some(record) = record {
        record
            .fields
            .update_slots(&arguments[1..], &ELEMENT_STYLE_SLOTS);
    }
}

fn update_relation_style(state: &mut ParseState, arguments: &[Argument]) {
    let (Some(from), Some(to)) = (arguments.first(), arguments.get(1)) else {
        return;
    };
    let (from, to) = (argument_text(from), argument_text(to));
    let record = state
        .relations
        .iter_mut()
        .find(|record| record.from == from && record.to == to);
    if let Some(record) = record {
        record
            .fields
            .update_slots(&arguments[2..], &RELATION_STYLE_SLOTS);
    }
}

/// c4Db.updateLayoutConfig: `$키`의 이름은 보지 않고 자리 순서로 (줄당 모양 수, 줄당 경계 수)를 읽는다.
/// 1보다 작거나 숫자가 아니면 그대로 둔다
fn update_layout_config(state: &mut ParseState, arguments: &[Argument]) {
    let count = |index: usize| {
        let argument = arguments.get(index)?;
        let value = parse_int(&argument_text(argument))?;
        usize::try_from(value).ok().filter(|value| *value >= 1)
    };
    if let Some(shapes) = count(0) {
        state.shapes_in_row = shapes;
    }
    if let Some(boundaries) = count(1) {
        state.boundaries_in_row = boundaries;
    }
}

/// 자리 인자는 그 글, `$키="값"`은 값
fn argument_text(argument: &Argument) -> String {
    match argument {
        Argument::Positional(value) | Argument::Named(_, value) => value.clone(),
    }
}

fn shape_style(fields: &Fields) -> ShapeStyle {
    ShapeStyle {
        background: fields.raw("bgColor"),
        font: fields.raw("fontColor"),
        border: fields.raw("borderColor"),
    }
}

/// 칸을 그리는 쪽이 읽는 값으로 바꾼다. TS가 그리다 멈추는 칸이 있으면 문법 오류다
fn finish(state: ParseState, diagram_type: &str) -> Result<C4Diagram, DiagramError> {
    let mut diagram = C4Diagram {
        diagram_type: diagram_type.to_string(),
        title: state.title,
        shapes_in_row: state.shapes_in_row,
        boundaries_in_row: state.boundaries_in_row,
        ..C4Diagram::default()
    };
    for record in state.elements {
        record
            .fields
            .validate_text_slots(&["label", "type", "techn", "descr"])?;
        diagram.elements.push(finish_element(record));
    }
    for record in state.boundaries {
        record
            .fields
            .validate_text_slots(&["label", "type", "descr"])?;
        diagram.boundaries.push(finish_boundary(record));
    }
    for record in state.relations {
        record
            .fields
            .validate_text_slots(&["label", "techn", "descr"])?;
        diagram.relations.push(finish_relation(record));
    }
    Ok(diagram)
}

fn finish_element(record: Record) -> C4Element {
    let fields = &record.fields;
    C4Element {
        label: fields.text("label"),
        techn: fields.text("techn"),
        descr: fields.text("descr"),
        has_sprite: fields.is_truthy("sprite"),
        style: shape_style(fields),
        kind: record.kind,
        alias: record.alias,
        parent: record.parent,
    }
}

fn finish_boundary(record: Record) -> C4Boundary {
    let fields = &record.fields;
    C4Boundary {
        label: fields.text("label"),
        kind: fields.text("type"),
        descr: fields.text("descr"),
        is_deployment_node: record.kind == "node",
        has_sprite: fields.is_truthy("sprite"),
        style: shape_style(fields),
        alias: record.alias,
        parent: record.parent,
    }
}

fn finish_relation(record: RelationRecord) -> C4Relation {
    let fields = &record.fields;
    let offset = |name: &str| {
        let value = fields.raw(name).and_then(|value| parse_int(&value));
        value.unwrap_or(0) as f64
    };
    C4Relation {
        label: fields.text("label"),
        techn: fields.text("techn"),
        text_color: fields.raw("textColor"),
        line_color: fields.raw("lineColor"),
        offset_x: offset("offsetX"),
        offset_y: offset("offsetY"),
        kind: record.kind,
        from: record.from,
        to: record.to,
    }
}

/// 쉼표로 인자를 나눈다. 따옴표 안 쉼표는 나누지 않는다
fn split_arguments(text: &str) -> Vec<Argument> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut is_quoted = false;
    for character in text.chars() {
        match character {
            '"' => {
                is_quoted = !is_quoted;
                current.push(character);
            }
            ',' if !is_quoted => parts.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    parts.push(current);
    parts
        .iter()
        .map(|part| parse_argument(part.trim()))
        .collect()
}

fn parse_argument(part: &str) -> Argument {
    if let Some(named) = part.strip_prefix('$')
        && let Some((key, value)) = named.split_once('=')
    {
        return Argument::Named(key.trim().to_string(), unquote(value.trim()));
    }
    Argument::Positional(unquote(part))
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 요소_경계_관계를_읽는다() {
        let diagram = parse(
            "C4Container\n  title T\n  Person(u, \"User\", \"desc\")\n  System_Boundary(b, \"Box\") {\n    Container(api, \"API\", \"Rust\", $descr=\"serves, data\")\n  }\n  Rel(u, api, \"Uses\", \"HTTPS\")\n",
        )
        .unwrap();
        assert_eq!(diagram.title, "T");
        assert_eq!(diagram.elements[0].kind, "person");
        assert_eq!(diagram.elements[0].descr, "desc");
        assert_eq!(diagram.elements[1].parent, "b");
        assert_eq!(diagram.elements[1].techn, "Rust");
        assert_eq!(diagram.elements[1].descr, "serves, data");
        assert_eq!(diagram.boundaries[1].kind, "SYSTEM");
        assert_eq!(diagram.relations[0].techn, "HTTPS");
    }

    #[test]
    fn 스타일_갱신과_배치_설정을_읽는다() {
        let diagram = parse(
            "C4Context\n  Person(user, \"User\")\n  Enterprise_Boundary(corp, \"Corp\") {\n    System(nas, \"NAS\")\n  }\n  Rel(user, nas, \"Uses\")\n  UpdateElementStyle(user, $fontColor=\"red\", $bgColor=\"grey\")\n  UpdateElementStyle(corp, $borderColor=\"#3355aa\")\n  UpdateElementStyle(nas, $sprite=\"disk\")\n  UpdateRelStyle(user, nas, $textColor=\"blue\", $offsetY=\"-20\")\n  UpdateLayoutConfig($c4ShapeInRow=\"2\", $c4BoundaryInRow=\"1\")\n",
        )
        .unwrap();
        let user = &diagram.elements[0];
        assert_eq!(user.style.font.as_deref(), Some("red"));
        assert_eq!(user.style.background.as_deref(), Some("grey"));
        assert_eq!(
            diagram.boundaries[1].style.border.as_deref(),
            Some("#3355aa")
        );
        assert_eq!(diagram.boundaries[1].kind, "ENTERPRISE");
        assert!(diagram.elements[1].has_sprite);
        let relation = &diagram.relations[0];
        assert_eq!(relation.text_color.as_deref(), Some("blue"));
        assert_eq!((relation.offset_x, relation.offset_y), (0.0, -20.0));
        assert_eq!((diagram.shapes_in_row, diagram.boundaries_in_row), (2, 1));
    }

    #[test]
    fn 스프라이트_태그_링크는_칸만_채운다() {
        let diagram = parse(
            "C4Container\n  Person(dev, \"Dev\", \"Uses\", $sprite=\"person2\", $link=\"https://x\")\n  Container(api, \"API\", $descr=\"Serves\", $link=\"https://x\")\n  Container(cli, \"CLI\", $descr=\"Dropped\")\n  Boundary(phone, \"Phone\", \"APP\", $sprite=\"phone\") {\n  }\n",
        )
        .unwrap();
        assert!(diagram.elements[0].has_sprite);
        assert_eq!(diagram.elements[1].descr, "Serves");
        assert_eq!(diagram.elements[1].techn, "");
        assert_eq!(diagram.elements[2].descr, "");
        assert!(diagram.boundaries[1].has_sprite);
        assert_eq!(diagram.boundaries[1].kind, "APP");
    }

    #[test]
    fn 요소_기술을_스타일로_바꾸면_ts처럼_오류다() {
        let result = parse(
            "C4Context\n  System(app, \"App\")\n  UpdateElementStyle(app, $techn=\"Rust\")\n",
        );
        assert!(matches!(result, Err(DiagramError::Syntax(_))));
    }
}
