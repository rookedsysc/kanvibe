//! wardley.langium 문법과 wardleyParser.ts의 populateDb를 줄 단위로 옮겼다.
//!
//! 좌표는 `[가시성, 진화]` 순서이고 1 이하면 비율(×100), 그보다 크면 백분율로 읽는다. 노드는 처음 나온
//! 순서를 지키며, 같은 이름이 다시 나오면 값만 덮어쓴다. 파이프라인 구성 요소는 `<부모>_<이름>` id를 받고,
//! 연결선은 id가 없으면 같은 라벨의 노드로 찾는다.

use crate::DiagramError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Anchor,
    Component,
    PipelineComponent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcingStrategy {
    Build,
    Buy,
    Outsource,
    Market,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapNode {
    pub id: String,
    pub label: String,
    /// 진화(가로) 백분율
    pub evolution: f64,
    /// 가시성(세로) 백분율
    pub visibility: f64,
    pub kind: NodeKind,
    pub label_offset_x: Option<f64>,
    pub label_offset_y: Option<f64>,
    pub has_inertia: bool,
    pub strategy: Option<SourcingStrategy>,
    pub is_pipeline_parent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkFlow {
    Forward,
    Backward,
    Bidirectional,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapLink {
    pub source: String,
    pub target: String,
    pub is_dashed: bool,
    pub label: Option<String>,
    pub flow: Option<LinkFlow>,
}

/// 이름과 (가로, 세로) 백분율 자리를 가진 표식(노트·가속·감속)
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedMark {
    pub text: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub number: u32,
    pub x: f64,
    pub y: f64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pipeline {
    pub parent: String,
    pub component_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WardleyMap {
    pub title: Option<String>,
    pub size: Option<(f64, f64)>,
    pub stages: Vec<String>,
    pub stage_boundaries: Vec<f64>,
    pub nodes: Vec<MapNode>,
    pub links: Vec<MapLink>,
    /// (노드 id, 목표 진화 백분율)
    pub trends: Vec<(String, f64)>,
    pub pipelines: Vec<Pipeline>,
    pub notes: Vec<PlacedMark>,
    pub annotations: Vec<Annotation>,
    pub annotations_box: Option<(f64, f64)>,
    pub accelerators: Vec<PlacedMark>,
    pub deaccelerators: Vec<PlacedMark>,
}

impl WardleyMap {
    pub fn node(&self, id: &str) -> Option<&MapNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    fn upsert_node(&mut self, node: MapNode) {
        match self
            .nodes
            .iter_mut()
            .find(|existing| existing.id == node.id)
        {
            Some(existing) => {
                let offset_x = node.label_offset_x.or(existing.label_offset_x);
                let offset_y = node.label_offset_y.or(existing.label_offset_y);
                *existing = MapNode {
                    label_offset_x: offset_x,
                    label_offset_y: offset_y,
                    ..node
                };
            }
            None => self.nodes.push(node),
        }
    }

    /// resolveNodeId: 같은 id, 없으면 같은 라벨
    fn resolve_node_id(&self, name: &str) -> String {
        if self.node(name).is_some() {
            return name.to_string();
        }
        self.nodes
            .iter()
            .find(|node| node.label == name)
            .map_or_else(|| name.to_string(), |node| node.id.clone())
    }
}

/// 문장 종류별로 모은 원문. populateDb는 종류별 순서(anchor → component → note → pipeline → link …)로 읽는다
#[derive(Default)]
struct Statements<'a> {
    anchors: Vec<&'a str>,
    components: Vec<&'a str>,
    notes: Vec<&'a str>,
    pipelines: Vec<(&'a str, Vec<&'a str>)>,
    links: Vec<&'a str>,
    evolves: Vec<&'a str>,
    annotations_box: Option<&'a str>,
    annotations: Vec<&'a str>,
    accelerators: Vec<&'a str>,
    deaccelerators: Vec<&'a str>,
}

pub fn parse(definition: &str) -> Result<WardleyMap, DiagramError> {
    let mut lines = definition
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    if lines.next() != Some("wardley-beta") {
        return Err(DiagramError::Syntax("wardley-beta 머리줄".to_string()));
    }
    let mut map = WardleyMap::default();
    let statements = collect_statements(&mut lines, &mut map)?;
    populate(&mut map, &statements)?;
    Ok(map)
}

fn collect_statements<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    map: &mut WardleyMap,
) -> Result<Statements<'a>, DiagramError> {
    let mut statements = Statements::default();
    while let Some(line) = lines.next() {
        let (keyword, rest) = split_keyword(line);
        match keyword {
            "title" => map.title = Some(rest.to_string()),
            "accTitle" | "accTitle:" | "accDescr" | "accDescr:" => {}
            "size" => map.size = Some(parse_pair(rest, line)?),
            "evolution" => read_evolution(rest, map),
            "anchor" => statements.anchors.push(rest),
            "component" => statements.components.push(rest),
            "note" => statements.notes.push(rest),
            "evolve" => statements.evolves.push(rest),
            "annotations" => statements.annotations_box = Some(rest),
            "annotation" => statements.annotations.push(rest),
            "accelerator" => statements.accelerators.push(rest),
            "deaccelerator" => statements.deaccelerators.push(rest),
            "pipeline" => statements.pipelines.push(read_pipeline_block(rest, lines)),
            _ => statements.links.push(line),
        }
    }
    Ok(statements)
}

/// 첫 낱말과 나머지. 낱말은 공백 앞까지다
fn split_keyword(line: &str) -> (&str, &str) {
    match line.split_once(char::is_whitespace) {
        Some((keyword, rest)) => (keyword, rest.trim()),
        None => (line, ""),
    }
}

/// `pipeline 부모 {` 다음 줄부터 `}`까지의 구성 요소 줄
fn read_pipeline_block<'a>(
    header: &'a str,
    lines: &mut impl Iterator<Item = &'a str>,
) -> (&'a str, Vec<&'a str>) {
    let parent = header.trim_end_matches('{').trim();
    let mut components = Vec::new();
    for line in lines.by_ref() {
        if line.starts_with('}') {
            break;
        }
        if let Some(rest) = line.strip_prefix("component") {
            components.push(rest.trim());
        }
    }
    (parent, components)
}

/// `evolution 이름[@경계][/ 둘째 이름] -> …`
fn read_evolution(rest: &str, map: &mut WardleyMap) {
    for stage in rest.split("->") {
        let (name_part, second_name) = match stage.split_once('/') {
            Some((name, second)) => (name, Some(unquote(second.trim()))),
            None => (stage, None),
        };
        let (name, boundary) = match name_part.split_once('@') {
            Some((name, boundary)) => (name, boundary.trim().parse::<f64>().ok()),
            None => (name_part, None),
        };
        let name = unquote(name.trim());
        map.stages.push(match second_name {
            Some(second) => format!("{name} / {second}"),
            None => name,
        });
        map.stage_boundaries.extend(boundary);
    }
}

fn populate(map: &mut WardleyMap, statements: &Statements) -> Result<(), DiagramError> {
    for rest in &statements.anchors {
        let (name, coordinates, _) = split_name_and_coordinates(rest)?;
        let (visibility, evolution) = parse_coordinates(coordinates)?;
        map.upsert_node(plain_node(name, NodeKind::Anchor, visibility, evolution));
    }
    for rest in &statements.components {
        map.upsert_node(parse_component(rest)?);
    }
    for rest in &statements.notes {
        let (name, coordinates, _) = split_name_and_coordinates(rest)?;
        let (visibility, evolution) = parse_coordinates(coordinates)?;
        map.notes.push(mark(name, evolution, visibility));
    }
    for (parent, components) in &statements.pipelines {
        add_pipeline(map, &unquote(parent), components)?;
    }
    for line in &statements.links {
        let link = parse_link(line, map)?;
        map.links.push(link);
    }
    for rest in &statements.evolves {
        read_evolve(rest, map)?;
    }
    populate_marks(map, statements)
}

fn populate_marks(map: &mut WardleyMap, statements: &Statements) -> Result<(), DiagramError> {
    if let Some(rest) = statements.annotations_box {
        let (x, y) = parse_pair(rest, rest)?;
        map.annotations_box = Some((to_percent(y)?, to_percent(x)?));
    }
    for rest in &statements.annotations {
        map.annotations.push(parse_annotation(rest)?);
    }
    for (sources, marks) in [
        (&statements.accelerators, &mut map.accelerators),
        (&statements.deaccelerators, &mut map.deaccelerators),
    ] {
        for rest in sources {
            let (name, coordinates, _) = split_name_and_coordinates(rest)?;
            let (x, y) = parse_coordinates(coordinates)?;
            marks.push(mark(name, y, x));
        }
    }
    Ok(())
}

fn mark(text: &str, x: f64, y: f64) -> PlacedMark {
    PlacedMark {
        text: unquote(text),
        x,
        y,
    }
}

fn plain_node(name: &str, kind: NodeKind, visibility: f64, evolution: f64) -> MapNode {
    let label = unquote(name);
    MapNode {
        id: label.clone(),
        label,
        evolution,
        visibility,
        kind,
        label_offset_x: None,
        label_offset_y: None,
        has_inertia: false,
        strategy: None,
        is_pipeline_parent: false,
    }
}

/// `이름 [가시성, 진화] (label [x, y])? ((전략))? (inertia)?`
fn parse_component(rest: &str) -> Result<MapNode, DiagramError> {
    let (name, coordinates, tail) = split_name_and_coordinates(rest)?;
    let (visibility, evolution) = parse_coordinates(coordinates)?;
    let mut node = plain_node(name, NodeKind::Component, visibility, evolution);
    let (offset, tail) = parse_label_offset(tail)?;
    node.label_offset_x = offset.map(|(x, _)| x);
    node.label_offset_y = offset.map(|(_, y)| y);
    node.strategy = ["build", "buy", "outsource", "market"]
        .into_iter()
        .zip([
            SourcingStrategy::Build,
            SourcingStrategy::Buy,
            SourcingStrategy::Outsource,
            SourcingStrategy::Market,
        ])
        .find(|(word, _)| tail.contains(&format!("({word})")))
        .map(|(_, strategy)| strategy);
    node.has_inertia = tail.contains("inertia");
    Ok(node)
}

/// 라벨 오프셋 (x, y)
type LabelOffset = (f64, f64);

/// `label [x, y]` 뒤에 남은 글자와 오프셋
fn parse_label_offset(tail: &str) -> Result<(Option<LabelOffset>, &str), DiagramError> {
    let Some(after) = tail.trim().strip_prefix("label") else {
        return Ok((None, tail));
    };
    let end = after
        .find(']')
        .ok_or_else(|| DiagramError::Syntax(format!("wardley label: {tail}")))?;
    let offset = parse_pair(&after[..=end], tail)?;
    Ok((Some(offset), &after[end + 1..]))
}

fn add_pipeline(
    map: &mut WardleyMap,
    parent: &str,
    components: &[&str],
) -> Result<(), DiagramError> {
    let parent_visibility = map
        .node(parent)
        .map(|node| node.visibility)
        .ok_or_else(|| DiagramError::Syntax(format!("wardley pipeline 부모 없음: {parent}")))?;
    if let Some(node) = map.nodes.iter_mut().find(|node| node.id == parent) {
        node.is_pipeline_parent = true;
    }
    let mut pipeline = Pipeline {
        parent: parent.to_string(),
        component_ids: Vec::new(),
    };
    for rest in components {
        let (name, coordinates, tail) = split_name_and_coordinates(rest)?;
        let evolution = to_percent(parse_number(coordinates.trim_matches(['[', ']']))?)?;
        let mut node = plain_node(
            name,
            NodeKind::PipelineComponent,
            parent_visibility,
            evolution,
        );
        node.id = format!("{parent}_{}", node.label);
        let (offset, _) = parse_label_offset(tail)?;
        node.label_offset_x = offset.map(|(x, _)| x);
        node.label_offset_y = offset.map(|(_, y)| y);
        pipeline.component_ids.push(node.id.clone());
        map.upsert_node(node);
    }
    map.pipelines.push(pipeline);
    Ok(())
}

fn read_evolve(rest: &str, map: &mut WardleyMap) -> Result<(), DiagramError> {
    let (name, target) = rest
        .rsplit_once(char::is_whitespace)
        .ok_or_else(|| DiagramError::Syntax(format!("wardley evolve: {rest}")))?;
    let name = unquote(name.trim());
    if map.node(&name).is_some() {
        let target = to_percent(parse_number(target)?)?;
        match map.trends.iter_mut().find(|(id, _)| *id == name) {
            Some(trend) => trend.1 = target,
            None => map.trends.push((name, target)),
        }
    }
    Ok(())
}

/// `번호,[x, y] "글"`
fn parse_annotation(rest: &str) -> Result<Annotation, DiagramError> {
    let invalid = || DiagramError::Syntax(format!("wardley annotation: {rest}"));
    let (number, after) = rest.split_once(',').ok_or_else(invalid)?;
    let close = after.find(']').ok_or_else(invalid)?;
    let (x, y) = parse_pair(&after[..=close], rest)?;
    Ok(Annotation {
        number: number.trim().parse().map_err(|_| invalid())?,
        x: to_percent(y)?,
        y: to_percent(x)?,
        text: unquote(after[close + 1..].trim()),
    })
}

/// `이름 [a, b] 꼬리` → (이름, `[a, b]`, 꼬리)
fn split_name_and_coordinates(rest: &str) -> Result<(&str, &str, &str), DiagramError> {
    let invalid = || DiagramError::Syntax(format!("wardley 좌표: {rest}"));
    let open = rest.rfind('[').ok_or_else(invalid)?;
    let open = match rest[..open].trim_end().strip_suffix("label") {
        Some(_) => rest[..open].rfind('[').ok_or_else(invalid)?,
        None => open,
    };
    let close = open + rest[open..].find(']').ok_or_else(invalid)?;
    Ok((rest[..open].trim(), &rest[open..=close], &rest[close + 1..]))
}

/// `[가시성, 진화]` → 백분율
fn parse_coordinates(text: &str) -> Result<(f64, f64), DiagramError> {
    let (visibility, evolution) = parse_pair(text, text)?;
    Ok((to_percent(visibility)?, to_percent(evolution)?))
}

fn parse_pair(text: &str, context: &str) -> Result<(f64, f64), DiagramError> {
    let inner = text.trim().trim_start_matches('[').trim_end_matches(']');
    let (first, second) = inner
        .split_once(',')
        .ok_or_else(|| DiagramError::Syntax(format!("wardley 좌표 쌍: {context}")))?;
    Ok((parse_number(first)?, parse_number(second)?))
}

fn parse_number(text: &str) -> Result<f64, DiagramError> {
    text.trim()
        .parse::<f64>()
        .map_err(|_| DiagramError::Syntax(format!("wardley 숫자: {text}")))
}

/// toPercent: 1 이하는 비율로 보고 100을 곱한다
fn to_percent(value: f64) -> Result<f64, DiagramError> {
    let normalized = if value <= 1.0 { value * 100.0 } else { value };
    match (0.0..=100.0).contains(&normalized) {
        true => Ok(normalized),
        false => Err(DiagramError::Syntax(format!(
            "wardley 백분율 범위: {value}"
        ))),
    }
}

fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    let is_quoted = trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')));
    match is_quoted {
        true => trimmed[1..trimmed.len() - 1].to_string(),
        false => trimmed.to_string(),
    }
}

/// 연결선 기호. 앞에서부터 처음 만나는 기호로 출발·도착 이름을 나눈다
const CONNECTORS: [&str; 5] = ["-.->", "-->", "->", "+", ">"];

/// `출발 [포트] [화살표] 도착 [포트] [; 라벨]`
fn parse_link(line: &str, map: &WardleyMap) -> Result<MapLink, DiagramError> {
    let (body, annotation) = match line.split_once(';') {
        Some((body, label)) => (body, Some(label.trim().to_string())),
        None => (line, None),
    };
    let invalid = || DiagramError::Syntax(format!("wardley 문장: {line}"));
    let start = (0..body.len())
        .filter(|index| body.is_char_boundary(*index))
        .find(|index| {
            CONNECTORS
                .iter()
                .any(|token| body[*index..].starts_with(token))
        })
        .ok_or_else(invalid)?;
    let source = unquote(&body[..start]);
    let (connector, rest) = split_connector(&body[start..]);
    let (target, target_port) = match rest.find('+') {
        Some(index) => (unquote(&rest[..index]), Some(rest[index..].trim())),
        None => (unquote(rest), None),
    };
    if source.is_empty() || target.is_empty() {
        return Err(invalid());
    }
    let (arrow_flow, arrow_label) = arrow_flow(connector);
    Ok(MapLink {
        source: map.resolve_node_id(&source),
        target: map.resolve_node_id(&target),
        is_dashed: connector.contains("-.->") || connector.contains(".-."),
        label: arrow_label.or(annotation),
        flow: port_flow(connector)
            .or_else(|| target_port.and_then(port_flow))
            .or(arrow_flow),
    })
}

/// 연결 기호 덩어리(포트·`+'라벨'` 화살표)와 그 뒤의 도착 이름
fn split_connector(text: &str) -> (&str, &str) {
    let mut end = 0;
    if let Some(after) = text.strip_prefix("+'") {
        end = 2 + after.find('\'').map_or(after.len(), |index| index + 1);
    }
    let symbols = ['+', '-', '.', '<', '>'];
    end += text[end..].len() - text[end..].trim_start_matches(symbols).len();
    (text[..end].trim(), &text[end..])
}

/// getFlowFromPort: 포트 기호(`+<>`·`+<`·`+>`)만으로 이뤄진 덩어리
fn port_flow(token: &str) -> Option<LinkFlow> {
    match token.trim() {
        "+<>" => Some(LinkFlow::Bidirectional),
        "+<" => Some(LinkFlow::Backward),
        "+>" => Some(LinkFlow::Forward),
        _ => None,
    }
}

/// extractFlowFromArrow: `+'라벨'>` 꼴 화살표의 방향과 라벨
fn arrow_flow(arrow: &str) -> (Option<LinkFlow>, Option<String>) {
    let Some(after_plus) = arrow.strip_prefix('+') else {
        return (None, None);
    };
    let label = after_plus
        .strip_prefix('\'')
        .and_then(|quoted| quoted.split_once('\''))
        .map(|(label, _)| label.to_string());
    let flow = if arrow.contains("<>") {
        Some(LinkFlow::Bidirectional)
    } else if arrow.contains('<') {
        Some(LinkFlow::Backward)
    } else if arrow.contains('>') {
        Some(LinkFlow::Forward)
    } else {
        None
    };
    (flow, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 좌표는_가시성_진화_순서의_백분율이다() {
        let map =
            parse("wardley-beta\ncomponent Cup of Tea [0.79, 0.61]\nanchor User [95, 50]").unwrap();
        let cup = map.node("Cup of Tea").unwrap();
        assert!((cup.visibility - 79.0).abs() < 1e-9);
        assert!((cup.evolution - 61.0).abs() < 1e-9);
        assert_eq!(map.nodes[0].kind, NodeKind::Anchor);
    }

    #[test]
    fn 연결선의_포트와_라벨_화살표를_읽는다() {
        let map = parse(
            "wardley-beta\ncomponent A [0.5, 0.5]\ncomponent B c [0.4, 0.4]\nA +'checkout'> B c\nA -.-> B c\nA +<> B c\nB c -> A; uses",
        )
        .unwrap();
        assert_eq!(map.links[0].flow, Some(LinkFlow::Forward));
        assert_eq!(map.links[0].label.as_deref(), Some("checkout"));
        assert_eq!(map.links[0].target, "B c");
        assert!(map.links[1].is_dashed);
        assert_eq!(map.links[2].flow, Some(LinkFlow::Bidirectional));
        assert_eq!(map.links[3].label.as_deref(), Some("uses"));
        assert_eq!(map.links[3].flow, None);
    }

    #[test]
    fn 파이프라인_구성_요소는_부모_높이에_놓인다() {
        let map = parse(
            "wardley-beta\ncomponent Data Store [0.20, 0.40]\npipeline Data Store {\n  component Files [0.25]\n}\nSearch -> Files",
        )
        .unwrap();
        let files = map.node("Data Store_Files").unwrap();
        assert!((files.visibility - 20.0).abs() < 1e-9);
        assert!(map.node("Data Store").unwrap().is_pipeline_parent);
        assert_eq!(map.links[0].target, "Data Store_Files");
    }

    #[test]
    fn 진화_단계의_경계와_둘째_이름을_읽는다() {
        let map = parse("wardley-beta\nevolution A@0.2 -> Good / Product -> C@1.0").unwrap();
        assert_eq!(map.stages, vec!["A", "Good / Product", "C"]);
        assert_eq!(map.stage_boundaries, vec![0.2, 1.0]);
    }
}
