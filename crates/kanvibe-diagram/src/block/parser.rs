//! block.jison 문법. 문장은 공백·줄바꿈으로 나뉘고 한 줄에 여럿 올 수 있다.
//!
//! 블록은 `id`, `id["이름"]`처럼 이름표 기호로 감싼 따옴표 이름, 뒤에 `:칸수`를 붙여 여러 칸을 차지한다. 화살표
//! 블록은 `id<["이름"]>(right)`이다. 간선은 `a --> b`, 이름 붙은 간선은 `a -- "이름" --> b`다. `columns N`,
//! `space(:N)`, `block(:id) … end` 묶음, `classDef`·`class`·`style`도 읽는다. jison 어휘 분석기처럼 기호는 규칙
//! 순서대로 처음 맞는 것을 고른다.

use crate::DiagramError;

/// 노드 하나: 이름표 기호를 이어 붙인 `typeStr`(예: `[]`, `(())`, `<[]>`)과 화살표 방향
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NodeSpec {
    pub id: String,
    pub label: Option<String>,
    pub type_str: Option<String>,
    pub directions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Statement {
    Node {
        node: NodeSpec,
        width_in_columns: u32,
    },
    /// `link`는 간선 끝 기호(`-->`, `==>`, `-.->` 등)다
    Edge {
        from: NodeSpec,
        to: NodeSpec,
        link: String,
        label: String,
    },
    /// `a --> b --> c`처럼 이어 쓴 간선. 원본 문법 동작은 앞 간선 결과(배열)를 노드로 읽어 id가 `undefined`인
    /// 빈 블록과 시작이 없는(그리지 않는) 간선, 마지막 노드만 남긴다
    BrokenChain {
        last: NodeSpec,
    },
    Space(u32),
    /// `columns auto`는 -1
    Columns(i64),
    /// `block:id … end`면 머리 노드와 칸수가 있다
    Composite {
        head: Option<(NodeSpec, u32)>,
        children: Vec<Statement>,
    },
    ClassDef {
        name: String,
        css: String,
    },
    ApplyClass {
        ids: String,
        class_name: String,
    },
    ApplyStyles {
        ids: String,
        styles: String,
    },
}

/// INITIAL 상태의 NODE_DSTART 규칙(54–72) 순서
const NODE_STARTS: [&str; 19] = [
    "-)", "(-", "))", ")", "(((", "((", "{{", "{", ">", "([", "(", "[[", "[|", "[(", ")))", "[\\",
    "[/", "[\\", "[",
];
/// NODE 상태의 NODE_DEND 규칙(37–53) 순서
const NODE_ENDS: [&str; 17] = [
    "(((", ")))", "))", "}}", "}", "(-", "-)", "((", "]]", "(", "])", "\\]", "/]", ")]", ")", "]>",
    "]",
];
const ARROW_DIRECTIONS: [&str; 6] = ["right", "left", "x", "y", "up", "down"];

fn syntax(message: impl Into<String>) -> DiagramError {
    DiagramError::Syntax(message.into())
}

/// 머리줄(`block-beta`·`block`) 뒤 문서를 읽는다. `%%` 주석 줄은 Mermaid 전처리처럼 먼저 지운다
pub(crate) fn parse(body: &str) -> Result<Vec<Statement>, DiagramError> {
    let cleaned: String = body
        .lines()
        .filter(|line| !line.trim_start().starts_with("%%"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut cursor = Cursor {
        text: cleaned.trim_start(),
        position: 0,
    };
    let keyword = cursor.take_while(|c| !c.is_whitespace());
    if keyword != "block-beta" && keyword != "block" {
        return Err(syntax(format!("block 머리줄: {keyword}")));
    }
    cursor.parse_document(false)
}

struct Cursor<'a> {
    text: &'a str,
    position: usize,
}

impl Cursor<'_> {
    fn rest(&self) -> &str {
        &self.text[self.position..]
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.text.len()
    }

    fn eat(&mut self, prefix: &str) -> bool {
        let is_match = self.rest().starts_with(prefix);
        if is_match {
            self.position += prefix.len();
        }
        is_match
    }

    fn take_while(&mut self, predicate: impl Fn(char) -> bool) -> String {
        let length = self
            .rest()
            .char_indices()
            .find(|(_, c)| !predicate(*c))
            .map_or(self.rest().len(), |(index, _)| index);
        let taken = self.rest()[..length].to_string();
        self.position += length;
        taken
    }

    fn take_line(&mut self) -> String {
        self.take_while(|c| c != '\n').trim().to_string()
    }

    fn skip_spaces(&mut self) {
        self.take_while(|c| c == ' ' || c == '\t');
    }

    fn skip_separators(&mut self) {
        self.take_while(char::is_whitespace);
    }

    /// 낱말 경계(`\b`)까지 맞는 예약어
    fn eat_word(&mut self, word: &str) -> bool {
        let rest = self.rest();
        let is_bounded = rest.starts_with(word)
            && !rest[word.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if is_bounded {
            self.position += word.len();
        }
        is_bounded
    }

    /// 예약어 뒤에 공백이 하나 이상 와야 하는 규칙(`classDef\s+` 등)
    fn eat_keyword_with_space(&mut self, word: &str) -> bool {
        let rest = self.rest();
        let is_match = rest.starts_with(word)
            && rest[word.len()..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace);
        if is_match {
            self.position += word.len();
            self.skip_spaces();
        }
        is_match
    }

    fn parse_document(&mut self, is_nested: bool) -> Result<Vec<Statement>, DiagramError> {
        let mut statements = Vec::new();
        loop {
            self.skip_separators();
            if self.is_at_end() {
                return if is_nested {
                    Err(syntax("block 묶음에 end가 없습니다"))
                } else {
                    Ok(statements)
                };
            }
            if self.eat_word("end") {
                return if is_nested {
                    Ok(statements)
                } else {
                    Err(syntax("짝이 없는 end"))
                };
            }
            if let Some(statement) = self.parse_statement()? {
                statements.push(statement);
            }
        }
    }

    fn parse_statement(&mut self) -> Result<Option<Statement>, DiagramError> {
        if self.eat("accTitle") || self.eat("accDescr") {
            self.skip_accessibility();
            return Ok(None);
        }
        if let Some(statement) = self.parse_setting()? {
            return Ok(Some(statement));
        }
        if self.eat("block:") {
            let head = self.parse_node_with_size()?;
            let children = self.parse_document(true)?;
            return Ok(Some(Statement::Composite {
                head: Some(head),
                children,
            }));
        }
        if self.eat_word("block") {
            let children = self.parse_document(true)?;
            return Ok(Some(Statement::Composite {
                head: None,
                children,
            }));
        }
        self.parse_node_statement().map(Some)
    }

    fn skip_accessibility(&mut self) {
        self.skip_spaces();
        if self.eat("{") {
            self.take_while(|c| c != '}');
            self.eat("}");
        } else {
            self.take_line();
        }
    }

    /// `columns`, `space`, `classDef`, `class`, `style`
    fn parse_setting(&mut self) -> Result<Option<Statement>, DiagramError> {
        if self.eat_keyword_with_space("columns") {
            let value = self.take_while(|c| !c.is_whitespace());
            let columns = match value.as_str() {
                "auto" => -1,
                digits => digits
                    .parse()
                    .map_err(|_| syntax(format!("columns 값: {digits}")))?,
            };
            return Ok(Some(Statement::Columns(columns)));
        }
        if self.eat("space:") {
            let digits = self.take_while(|c| c.is_ascii_digit());
            let count = digits
                .parse()
                .map_err(|_| syntax(format!("space 칸수: {digits}")))?;
            return Ok(Some(Statement::Space(count)));
        }
        if self.eat_word("space") {
            return Ok(Some(Statement::Space(1)));
        }
        if self.eat_keyword_with_space("classDef") {
            let name = self.take_while(|c| !c.is_whitespace());
            let css = self.take_line();
            return Ok(Some(Statement::ClassDef { name, css }));
        }
        if self.eat_keyword_with_space("class") {
            let ids = self.take_id_list();
            self.skip_spaces();
            let class_name = self.take_line();
            return Ok(Some(Statement::ApplyClass { ids, class_name }));
        }
        if self.eat_keyword_with_space("style") {
            let ids = self.take_id_list();
            self.skip_spaces();
            let styles = self.take_line();
            return Ok(Some(Statement::ApplyStyles { ids, styles }));
        }
        Ok(None)
    }

    /// `(\w+)+((,\s*\w+)*)`
    fn take_id_list(&mut self) -> String {
        let mut ids = self.take_while(|c| c.is_alphanumeric() || c == '_');
        while self.rest().starts_with(',') {
            let before = self.position;
            self.position += 1;
            self.skip_spaces();
            let next = self.take_while(|c| c.is_alphanumeric() || c == '_');
            if next.is_empty() {
                self.position = before;
                break;
            }
            ids.push(',');
            ids.push_str(&next);
        }
        ids
    }

    /// nodeStatement: `node`, `node :칸수`, `node 간선 node`
    fn parse_node_statement(&mut self) -> Result<Statement, DiagramError> {
        let node = self.parse_node()?;
        if let Some(width_in_columns) = self.parse_size()? {
            return Ok(Statement::Node {
                node,
                width_in_columns,
            });
        }
        let checkpoint = self.position;
        self.skip_spaces();
        let Some((link, label)) = self.parse_link()? else {
            self.position = checkpoint;
            return Ok(Statement::Node {
                node,
                width_in_columns: 1,
            });
        };
        self.skip_spaces();
        let to = self.parse_node()?;
        if let Some(last) = self.parse_chain_tail()? {
            return Ok(Statement::BrokenChain { last });
        }
        Ok(Statement::Edge {
            from: node,
            to,
            link,
            label,
        })
    }

    /// 간선 뒤에 간선이 더 이어지면 마지막 노드를 돌려준다
    fn parse_chain_tail(&mut self) -> Result<Option<NodeSpec>, DiagramError> {
        let mut last = None;
        loop {
            let checkpoint = self.position;
            self.skip_spaces();
            if self.parse_link()?.is_none() {
                self.position = checkpoint;
                return Ok(last);
            }
            self.skip_spaces();
            last = Some(self.parse_node()?);
        }
    }

    fn parse_node_with_size(&mut self) -> Result<(NodeSpec, u32), DiagramError> {
        let node = self.parse_node()?;
        let width_in_columns = self.parse_size()?.unwrap_or(1);
        Ok((node, width_in_columns))
    }

    /// SIZE: `:\d+`
    fn parse_size(&mut self) -> Result<Option<u32>, DiagramError> {
        let rest = self.rest();
        let is_size =
            rest.starts_with(':') && rest[1..].chars().next().is_some_and(|c| c.is_ascii_digit());
        if !is_size {
            return Ok(None);
        }
        self.position += 1;
        let digits = self.take_while(|c| c.is_ascii_digit());
        digits
            .parse()
            .map(Some)
            .map_err(|_| syntax(format!("칸수: {digits}")))
    }

    /// NODE_ID(`[^\(\[\n\-\)\{\}\s\<\>:=]+`) 뒤에 이름표 기호가 붙을 수 있다
    fn parse_node(&mut self) -> Result<NodeSpec, DiagramError> {
        let id = self.take_while(|c| !"([\n-){}<>:=".contains(c) && !c.is_whitespace());
        if id.is_empty() {
            let found: String = self.rest().chars().take(12).collect();
            return Err(syntax(format!("block 문법: {found}")));
        }
        let mut node = NodeSpec {
            id,
            ..NodeSpec::default()
        };
        if self.eat("<[") {
            let label = self.parse_quoted()?;
            node.directions = self.parse_arrow_directions()?;
            node.label = Some(label);
            node.type_str = Some("<[]>".to_string());
            return Ok(node);
        }
        let Some(start) = NODE_STARTS.iter().find(|start| self.eat(start)) else {
            return Ok(node);
        };
        let label = self.parse_quoted()?;
        let end = NODE_ENDS
            .iter()
            .find(|end| self.eat(end))
            .ok_or_else(|| syntax(format!("{} 이름표가 닫히지 않습니다", node.id)))?;
        node.label = Some(label);
        node.type_str = Some(format!("{start}{end}"));
        Ok(node)
    }

    /// `"글자"`. 마크다운 `"`글자`"`는 어휘 분석기가 문법에 없는 MD_STR 토큰을 내서 원본도 문법 오류다
    fn parse_quoted(&mut self) -> Result<String, DiagramError> {
        if self.rest().starts_with("\"`") {
            return Err(syntax("block 다이어그램은 마크다운 글자를 받지 않습니다"));
        }
        if !self.eat("\"") {
            return Err(syntax("이름은 따옴표 글자여야 합니다"));
        }
        let text = self.take_while(|c| c != '"');
        self.eat("\"");
        Ok(text)
    }

    /// `]>(right, down)`: 쉼표·공백으로 나눈 방향 목록
    fn parse_arrow_directions(&mut self) -> Result<Vec<String>, DiagramError> {
        if !self.eat("]>") {
            return Err(syntax("화살표 블록은 ]>(방향)으로 닫아야 합니다"));
        }
        self.skip_spaces();
        if !self.eat("(") {
            return Err(syntax("화살표 블록 방향이 없습니다"));
        }
        let mut directions = Vec::new();
        loop {
            self.eat(",");
            self.skip_separators();
            if self.eat(")") {
                return Ok(directions);
            }
            let direction = ARROW_DIRECTIONS
                .iter()
                .find(|direction| self.eat(direction))
                .ok_or_else(|| syntax("화살표 블록 방향"))?;
            directions.push(direction.to_string());
        }
    }

    /// LINK 또는 START_LINK + 따옴표 이름 + LINK. 간선 끝 기호와 이름을 돌려준다
    fn parse_link(&mut self) -> Result<Option<(String, String)>, DiagramError> {
        if let Some(link) = self.match_link() {
            return Ok(Some((link, String::new())));
        }
        let start = self.position;
        self.eat("<");
        let is_start_link = self.eat("--") || self.eat("==") || self.eat("-.");
        if !is_start_link {
            self.position = start;
            return Ok(None);
        }
        self.skip_spaces();
        let label = self.parse_quoted()?;
        self.skip_spaces();
        let link = self
            .match_link()
            .ok_or_else(|| syntax("이름 붙은 간선의 끝 기호가 없습니다"))?;
        Ok(Some((link, label)))
    }

    /// `[xo<]?--+[-xo>]`, `[xo<]?==+[=xo>]`, `[xo<]?-?\.+-[xo>]?`, `~~[~]+`. 앞의 `x`·`o`는 먼저 오는
    /// NODE_ID 규칙이 노드 id로 가져가므로 간선 앞머리로는 `<`만 남는다
    fn match_link(&mut self) -> Option<String> {
        let rest = self.rest();
        let length = link_length(rest)?;
        let link = rest[..length].to_string();
        self.position += length;
        Some(link)
    }
}

fn link_length(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let tilde = bytes.iter().take_while(|byte| **byte == b'~').count();
    if tilde >= 3 {
        return Some(tilde);
    }
    let prefix = usize::from(bytes.first() == Some(&b'<'));
    let body = &bytes[prefix..];
    let repeated = |symbol: u8| body.iter().take_while(|byte| **byte == symbol).count();
    for (symbol, ends) in [(b'-', &b"-xo>"[..]), (b'=', &b"=xo>"[..])] {
        let count = repeated(symbol);
        if count >= 3 && ends.contains(&symbol) {
            return Some(prefix + count);
        }
        if count >= 2 && body.get(count).is_some_and(|byte| ends.contains(byte)) {
            return Some(prefix + count + 1);
        }
    }
    dotted_link_length(body).map(|length| prefix + length)
}

/// `-?\.+-[xo>]?`
fn dotted_link_length(body: &[u8]) -> Option<usize> {
    let dash = usize::from(body.first() == Some(&b'-'));
    let dots = body[dash..]
        .iter()
        .take_while(|byte| **byte == b'.')
        .count();
    if dots == 0 || body.get(dash + dots) != Some(&b'-') {
        return None;
    }
    let tail = usize::from(
        body.get(dash + dots + 1)
            .is_some_and(|byte| b"xo>".contains(byte)),
    );
    Some(dash + dots + 1 + tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, label: &str, type_str: &str) -> NodeSpec {
        NodeSpec {
            id: id.to_string(),
            label: Some(label.to_string()),
            type_str: Some(type_str.to_string()),
            directions: Vec::new(),
        }
    }

    #[test]
    fn 한_줄에_여러_블록과_칸수를_읽는다() {
        let statements =
            parse("block-beta\n  columns 3\n  a[\"Frontend\"] b[\"Backend\"]:2 c((\"C\")) space:2")
                .unwrap();
        assert_eq!(statements[0], Statement::Columns(3));
        assert_eq!(
            statements[2],
            Statement::Node {
                node: node("b", "Backend", "[]"),
                width_in_columns: 2
            }
        );
        assert_eq!(
            statements[3],
            Statement::Node {
                node: node("c", "C", "(())"),
                width_in_columns: 1
            }
        );
        assert_eq!(statements[4], Statement::Space(2));
    }

    #[test]
    fn 이름표_기호는_규칙_순서대로_고른다() {
        let statements = parse("block\n  g[(\"DB\")] l[/\"L\"\\] r>\"R\"] s([\"S\"])").unwrap();
        let types: Vec<String> = statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Node { node, .. } => node.type_str.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(types, vec!["[()]", "[/\\]", ">]", "([])"]);
    }

    #[test]
    fn 간선과_이름_붙은_간선을_읽는다() {
        let statements =
            parse("block\n  a --> b\n  b -- \"query\" --> g\n  x ==o y\n  a o--o b").unwrap();
        let Statement::Edge { link, label, .. } = &statements[1] else {
            panic!("간선이어야 합니다");
        };
        assert_eq!((link.as_str(), label.as_str()), ("-->", "query"));
        let Statement::Edge { link, .. } = &statements[2] else {
            panic!("간선이어야 합니다");
        };
        assert_eq!(link, "==o");
        // `o--o`의 앞 `o`는 노드 id가 되고 간선은 `--o`다
        let Statement::Edge { from, link, .. } = &statements[4] else {
            panic!("간선이어야 합니다");
        };
        assert_eq!((from.id.as_str(), link.as_str()), ("o", "--o"));
    }

    #[test]
    fn 이어_쓴_간선은_마지막_노드만_남긴다() {
        let statements = parse("block\n  a --> b -- \"x\" --> c\n  d").unwrap();
        let Statement::BrokenChain { last } = &statements[0] else {
            panic!("이어 쓴 간선이어야 합니다");
        };
        assert_eq!(last.id, "c");
        assert!(matches!(&statements[1], Statement::Node { node, .. } if node.id == "d"));
    }

    #[test]
    fn 마크다운_이름은_문법_오류다() {
        // block-markdown 확인: TS 워커도 422 문법 오류로 그리지 못했다
        assert!(parse("block-beta\n  a[\"`**Bold**`\"]").is_err());
        assert!(parse("block-beta\n  a b\n  a -- \"`x`\" --> b").is_err());
    }

    #[test]
    fn 묶음과_화살표_블록과_스타일_문장을_읽는다() {
        let statements = parse(
            "block-beta\n  block:group1:2\n    columns 2\n    B\n  end\n  send<[\"Send\"]>(right, down)\n  classDef warm fill:#f96\n  class a,b warm\n  style out fill:#bbf",
        )
        .unwrap();
        let Statement::Composite { head, children } = &statements[0] else {
            panic!("묶음이어야 합니다");
        };
        assert_eq!(
            head.as_ref()
                .map(|(node, width)| (node.id.as_str(), *width)),
            Some(("group1", 2))
        );
        assert_eq!(children.len(), 2);
        let Statement::Node { node, .. } = &statements[1] else {
            panic!("화살표 블록이어야 합니다");
        };
        assert_eq!(node.directions, vec!["right", "down"]);
        assert_eq!(
            statements[3],
            Statement::ApplyClass {
                ids: "a,b".to_string(),
                class_name: "warm".to_string()
            }
        );
    }
}
