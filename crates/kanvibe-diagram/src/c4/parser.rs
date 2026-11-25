//! c4Diagram.jison과 c4Db.js의 요소·경계·관계 추가 규칙.
//!
//! 한 줄에 `키워드(인자, …)` 하나를 읽는다. 인자는 `"따옴표 글"`, 맨 글, `$키="값"`이다. `$키` 인자는 그 이름의
//! 칸(`$descr`, `$techn` 등)을 채운다. 경계는 줄 끝 `{`로 열고 `}`로 닫는다.

use crate::DiagramError;

/// 맨 바깥 경계 별칭(c4Db `boundaries[0]`)
pub(crate) const GLOBAL_BOUNDARY: &str = "global";

#[derive(Clone, Debug, Default)]
pub(crate) struct C4Element {
    /// `person`, `external_system_db` 같은 c4Db `typeC4Shape`
    pub kind: String,
    pub alias: String,
    pub label: String,
    pub techn: String,
    pub descr: String,
    pub parent: String,
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
}

#[derive(Clone, Debug, Default)]
pub(crate) struct C4Relation {
    /// `rel`, `birel`, `rel_u`, `rel_b` …
    pub kind: String,
    pub from: String,
    pub to: String,
    pub label: String,
    pub techn: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct C4Diagram {
    pub diagram_type: String,
    pub title: String,
    pub elements: Vec<C4Element>,
    pub boundaries: Vec<C4Boundary>,
    pub relations: Vec<C4Relation>,
}

/// 인자 하나: 자리 인자이거나 `$키="값"`
#[derive(Clone, Debug, PartialEq)]
enum Argument {
    Positional(String),
    Named(String, String),
}

pub(crate) fn parse(definition: &str) -> Result<C4Diagram, DiagramError> {
    let mut lines = definition
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines.next().unwrap_or_default();
    let mut diagram = C4Diagram {
        diagram_type: header
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string(),
        boundaries: vec![C4Boundary {
            alias: GLOBAL_BOUNDARY.to_string(),
            label: GLOBAL_BOUNDARY.to_string(),
            kind: GLOBAL_BOUNDARY.to_string(),
            ..C4Boundary::default()
        }],
        ..C4Diagram::default()
    };
    let mut parents: Vec<String> = vec![GLOBAL_BOUNDARY.to_string()];
    for line in lines {
        parse_statement(&mut diagram, &mut parents, line)?;
    }
    Ok(diagram)
}

fn parse_statement(
    diagram: &mut C4Diagram,
    parents: &mut Vec<String>,
    line: &str,
) -> Result<(), DiagramError> {
    if line == "}" {
        if parents.len() > 1 {
            parents.pop();
        }
        return Ok(());
    }
    if let Some(title) = line.strip_prefix("title ") {
        diagram.title = title.trim().to_string();
        return Ok(());
    }
    let Some(open) = line.find('(') else {
        return skip_or_reject(line);
    };
    let keyword = line[..open].trim();
    let close = line.rfind(')').unwrap_or(line.len());
    let arguments = split_arguments(&line[open + 1..close.max(open + 1)]);
    let parent = parents.last().cloned().unwrap_or_default();
    if let Some(kind) = element_kind(keyword) {
        diagram
            .elements
            .push(build_element(kind, parent, &arguments));
    } else if let Some(kind) = relation_kind(keyword) {
        add_relation(diagram, kind, keyword, &arguments);
    } else if let Some(boundary) = build_boundary(keyword, parent, &arguments) {
        parents.push(boundary.alias.clone());
        diagram.boundaries.push(boundary);
    }
    Ok(())
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

/// 자리 인자 순서: 사람·시스템은 (별칭, 이름, 설명), 컨테이너·컴포넌트는 (별칭, 이름, 기술, 설명)
fn build_element(kind: &str, parent: String, arguments: &[Argument]) -> C4Element {
    let has_techn = kind.contains("container") || kind.contains("component");
    let order: &[&str] = if has_techn {
        &["alias", "label", "techn", "descr"]
    } else {
        &["alias", "label", "descr"]
    };
    let field = |name: &str| argument_value(arguments, order, name);
    C4Element {
        kind: kind.to_string(),
        alias: field("alias"),
        label: field("label"),
        techn: field("techn"),
        descr: field("descr"),
        parent,
    }
}

/// `$이름` 인자가 있으면 그 값, 없으면 자리 인자 순서에서 찾는다
fn argument_value(arguments: &[Argument], order: &[&str], name: &str) -> String {
    let named = arguments.iter().find_map(|argument| match argument {
        Argument::Named(key, value) if key == name => Some(value.clone()),
        _ => None,
    });
    let positional: Vec<&String> = arguments
        .iter()
        .filter_map(|argument| match argument {
            Argument::Positional(value) => Some(value),
            Argument::Named(..) => None,
        })
        .collect();
    let index = order.iter().position(|field| *field == name);
    named
        .or_else(|| index.and_then(|index| positional.get(index).map(|value| (*value).clone())))
        .unwrap_or_default()
}

/// c4Db.addRel: 같은 (from, to) 관계는 마지막 정의로 덮어쓴다. RelIndex는 첫 인자(순번)를 버린다
fn add_relation(diagram: &mut C4Diagram, kind: &str, keyword: &str, arguments: &[Argument]) {
    let skipped = usize::from(keyword == "RelIndex");
    let arguments = &arguments[skipped.min(arguments.len())..];
    let order = ["from", "to", "label", "techn"];
    let field = |name: &str| argument_value(arguments, &order, name);
    let relation = C4Relation {
        kind: kind.to_string(),
        from: field("from"),
        to: field("to"),
        label: field("label"),
        techn: field("techn"),
    };
    let existing = diagram
        .relations
        .iter_mut()
        .find(|old| old.from == relation.from && old.to == relation.to);
    match existing {
        Some(old) => *old = relation,
        None => diagram.relations.push(relation),
    }
}

/// 경계: (별칭, 이름, 종류) 또는 배포 노드 (별칭, 이름, 종류, 설명)
fn build_boundary(keyword: &str, parent: String, arguments: &[Argument]) -> Option<C4Boundary> {
    let (default_kind, is_deployment_node) = match keyword {
        "Boundary" => ("system", false),
        "Enterprise_Boundary" => ("ENTERPRISE", false),
        "System_Boundary" => ("SYSTEM", false),
        "Container_Boundary" => ("CONTAINER", false),
        "Deployment_Node" | "Node" | "Node_L" | "Node_R" => ("node", true),
        _ => return None,
    };
    let is_fixed_kind = matches!(
        keyword,
        "Enterprise_Boundary" | "System_Boundary" | "Container_Boundary"
    );
    let order: &[&str] = if is_fixed_kind {
        &["alias", "label"]
    } else {
        &["alias", "label", "type", "descr"]
    };
    let field = |name: &str| argument_value(arguments, order, name);
    let kind = Some(field("type"))
        .filter(|value| !value.is_empty() && !is_fixed_kind)
        .unwrap_or_else(|| default_kind.to_string());
    Some(C4Boundary {
        alias: field("alias"),
        label: field("label"),
        kind,
        descr: if is_deployment_node {
            field("descr")
        } else {
            String::new()
        },
        is_deployment_node,
        parent,
    })
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
}
