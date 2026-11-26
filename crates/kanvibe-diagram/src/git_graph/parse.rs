//! gitGraph 문법(langium GitGraphGrammar)과 gitGraphAst.ts의 커밋·브랜치 기록.
//!
//! `commit`, `branch`, `checkout`/`switch`, `merge`, `cherry-pick`과 머리줄 방향(`LR`·`TB`·`BT`), `title`을
//! 읽는다. id 없는 커밋은 Mermaid가 무작위 7자리 16진수를 붙이지만, 여기서는 정의와 순번의 해시로
//! 같은 정의가 늘 같은 그림이 되게 한다. id·태그·브랜치 이름은 Mermaid처럼 DOMPurify를 거친 모양으로 둔다.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::options::GitGraphOptions;
use crate::DiagramError;
use crate::directive::{diagram_body, read_config};
use crate::sanitize::sanitize_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    TopToBottom,
    BottomToTop,
}

impl Direction {
    pub fn is_vertical(self) -> bool {
        self != Direction::LeftToRight
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitKind {
    Normal,
    Reverse,
    Highlight,
    Merge,
    CherryPick,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub id: String,
    pub seq: usize,
    pub kind: CommitKind,
    /// 그릴 모양(`merge … type:`이 있으면 그 값, 없으면 `kind`)
    pub symbol: CommitKind,
    /// `merge … id:`로 id를 직접 준 병합 커밋
    pub has_custom_merge_id: bool,
    pub tags: Vec<String>,
    pub parents: Vec<String>,
    pub branch: String,
}

#[derive(Clone, Debug)]
pub struct GitGraph {
    pub direction: Direction,
    pub title: String,
    /// 순번(seq) 순서
    pub commits: Vec<Commit>,
    /// `order` 순으로 정렬한 브랜치 이름(같으면 만든 순서)
    pub branches: Vec<String>,
    pub options: GitGraphOptions,
}

impl GitGraph {
    pub fn commit(&self, id: &str) -> Option<&Commit> {
        self.commits.iter().find(|commit| commit.id == id)
    }
}

/// 한 문장의 낱말. 따옴표 문자열은 따옴표를 벗긴다
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
}

impl Token {
    fn text(&self) -> &str {
        match self {
            Token::Word(text) | Token::Quoted(text) => text,
        }
    }
}

/// 문장을 처리하며 브랜치 머리와 현재 브랜치를 기억하는 기록부(gitGraphAst.ts state)
struct GitRecorder {
    definition_hash: Sha256,
    commits: Vec<Commit>,
    /// 브랜치 이름 → 머리 커밋 id. 순서는 만든 순서
    branch_heads: Vec<(String, Option<String>)>,
    branch_orders: HashMap<String, f64>,
    current_branch: String,
    head: Option<String>,
}

pub fn parse(definition: &str) -> Result<GitGraph, DiagramError> {
    let config = read_config(definition);
    let options = GitGraphOptions::from_config(&config);
    let body = diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    let direction = parse_header(header)?;
    let mut recorder = GitRecorder::new(definition, &options);
    let mut title = config
        .title
        .as_deref()
        .map(sanitize_text)
        .unwrap_or_default();
    for line in lines {
        if let Some(rest) = line.strip_prefix("title") {
            title = sanitize_text(rest.trim());
            continue;
        }
        if line.starts_with("accTitle") || line.starts_with("accDescr") {
            continue;
        }
        recorder.apply(&tokenize(line))?;
    }
    Ok(recorder.finish(direction, title, options))
}

/// `gitGraph`, `gitGraph:`, `gitGraph LR:` 등
fn parse_header(header: &str) -> Result<Direction, DiagramError> {
    let rest = header
        .strip_prefix("gitGraph")
        .ok_or_else(|| DiagramError::Syntax(format!("gitGraph 머리줄: {header}")))?;
    match rest.trim().trim_end_matches(':').trim() {
        "" | "LR" => Ok(Direction::LeftToRight),
        "TB" => Ok(Direction::TopToBottom),
        "BT" => Ok(Direction::BottomToTop),
        other => Err(DiagramError::Syntax(format!("gitGraph 방향: {other}"))),
    }
}

/// 공백으로 나누되 따옴표 문자열은 한 낱말로 둔다. `order:2`처럼 붙은 열쇠는 `order:`와 값으로 가른다
fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut characters = line.chars().peekable();
    while let Some(&character) = characters.peek() {
        if character.is_whitespace() {
            characters.next();
        } else if character == '"' || character == '\'' {
            characters.next();
            let quoted: String = characters
                .by_ref()
                .take_while(|c| *c != character)
                .collect();
            tokens.push(Token::Quoted(sanitize_text(&quoted)));
        } else {
            push_word(&mut tokens, read_word(&mut characters));
        }
    }
    tokens
}

/// 공백이나 따옴표 앞까지의 맨 낱말
fn read_word(characters: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let is_word_char = |c: &char| !c.is_whitespace() && *c != '"' && *c != '\'';
    let mut word = String::new();
    while let Some(next) = characters.next_if(is_word_char) {
        word.push(next);
    }
    word
}

fn push_word(tokens: &mut Vec<Token>, word: String) {
    match word.split_once(':') {
        Some((key, value)) if !value.is_empty() => {
            tokens.push(Token::Word(format!("{key}:")));
            tokens.push(Token::Word(sanitize_text(value)));
        }
        _ => tokens.push(Token::Word(sanitize_text(&word))),
    }
}

/// `열쇠: 값` 꼴 선택지들
#[derive(Default)]
struct Options {
    id: Option<String>,
    tags: Vec<String>,
    kind: Option<CommitKind>,
    order: Option<f64>,
    parent: Option<String>,
}

fn parse_options(tokens: &[Token]) -> Result<Options, DiagramError> {
    let mut options = Options::default();
    let mut index = 0;
    while index < tokens.len() {
        let key = tokens[index].text();
        let value = tokens.get(index + 1).map(Token::text);
        let invalid = || DiagramError::Syntax(format!("gitGraph 선택지: {key}"));
        index += 2;
        match (key, value) {
            ("id:", Some(value)) => options.id = Some(value.to_string()),
            ("msg:", Some(_)) => {}
            ("tag:", Some(value)) => options.tags.push(value.to_string()),
            ("type:", Some(value)) => options.kind = Some(parse_kind(value).ok_or_else(invalid)?),
            ("order:", Some(value)) => {
                options.order = Some(value.parse().map_err(|_| invalid())?);
            }
            ("parent:", Some(value)) => options.parent = Some(value.to_string()),
            // 열쇠 없는 따옴표 문자열은 커밋 메시지다
            _ if matches!(tokens[index - 2], Token::Quoted(_)) => index -= 1,
            _ => return Err(invalid()),
        }
    }
    Ok(options)
}

fn parse_kind(value: &str) -> Option<CommitKind> {
    match value {
        "NORMAL" => Some(CommitKind::Normal),
        "REVERSE" => Some(CommitKind::Reverse),
        "HIGHLIGHT" => Some(CommitKind::Highlight),
        _ => None,
    }
}

impl GitRecorder {
    fn new(definition: &str, options: &GitGraphOptions) -> Self {
        let main = options.main_branch_name.clone();
        let mut definition_hash = Sha256::new();
        definition_hash.update(definition.as_bytes());
        Self {
            definition_hash,
            commits: Vec::new(),
            branch_heads: vec![(main.clone(), None)],
            branch_orders: HashMap::from([(main.clone(), options.main_branch_order)]),
            current_branch: main,
            head: None,
        }
    }

    fn apply(&mut self, tokens: &[Token]) -> Result<(), DiagramError> {
        let Some((keyword, rest)) = tokens.split_first() else {
            return Ok(());
        };
        match keyword.text() {
            "commit" => self.commit(parse_options(rest)?),
            "branch" => self.branch(rest),
            "checkout" | "switch" => self.checkout(name_argument(rest)?),
            "merge" => self.merge(rest),
            "cherry-pick" => self.cherry_pick(parse_options(rest)?),
            other => Err(DiagramError::Syntax(format!("gitGraph 문장: {other}"))),
        }
    }

    /// `seq-` 뒤에 정의와 순번에서 얻은 7자리 16진수를 붙인다
    fn generated_id(&self) -> String {
        let seq = self.commits.len();
        let mut hash = self.definition_hash.clone();
        hash.update(seq.to_le_bytes());
        let digest = hash.finalize();
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("{seq}-{}", &hex[..7])
    }

    fn push_commit(&mut self, commit: Commit) {
        self.head = Some(commit.id.clone());
        let current = self.current_branch.clone();
        self.set_branch_head(&current, commit.id.clone());
        self.commits.push(commit);
    }

    fn set_branch_head(&mut self, name: &str, id: String) {
        if let Some((_, head)) = self
            .branch_heads
            .iter_mut()
            .find(|(branch, _)| branch == name)
        {
            *head = Some(id);
        }
    }

    fn branch_head(&self, name: &str) -> Option<Option<&String>> {
        self.branch_heads
            .iter()
            .find(|(branch, _)| branch == name)
            .map(|(_, head)| head.as_ref())
    }

    fn commit(&mut self, options: Options) -> Result<(), DiagramError> {
        let kind = options.kind.unwrap_or(CommitKind::Normal);
        let commit = Commit {
            id: options.id.unwrap_or_else(|| self.generated_id()),
            seq: self.commits.len(),
            kind,
            symbol: kind,
            has_custom_merge_id: false,
            tags: options.tags,
            parents: self.head.iter().cloned().collect(),
            branch: self.current_branch.clone(),
        };
        self.push_commit(commit);
        Ok(())
    }

    fn branch(&mut self, rest: &[Token]) -> Result<(), DiagramError> {
        let name = name_argument(rest)?.to_string();
        let options = parse_options(&rest[1..])?;
        if self.branch_head(&name).is_some() {
            return Err(DiagramError::Syntax(format!("이미 있는 브랜치: {name}")));
        }
        self.branch_heads.push((name.clone(), self.head.clone()));
        self.branch_orders
            .insert(name.clone(), options.order.unwrap_or(0.0));
        self.checkout(&name)
    }

    fn checkout(&mut self, name: &str) -> Result<(), DiagramError> {
        let head = self
            .branch_head(name)
            .ok_or_else(|| DiagramError::Syntax(format!("없는 브랜치로 checkout: {name}")))?
            .cloned();
        self.current_branch = name.to_string();
        self.head = head;
        Ok(())
    }

    fn merge(&mut self, rest: &[Token]) -> Result<(), DiagramError> {
        let other = name_argument(rest)?.to_string();
        let options = parse_options(&rest[1..])?;
        let merge_error = |reason: &str| DiagramError::Syntax(format!("merge {other}: {reason}"));
        if other == self.current_branch {
            return Err(merge_error("자기 자신과 병합"));
        }
        let current_head = self
            .head
            .clone()
            .ok_or_else(|| merge_error("현재 브랜치에 커밋 없음"))?;
        let other_head = self
            .branch_head(&other)
            .ok_or_else(|| merge_error("없는 브랜치"))?
            .cloned()
            .ok_or_else(|| merge_error("병합할 브랜치에 커밋 없음"))?;
        if current_head == other_head {
            return Err(merge_error("두 브랜치의 머리가 같음"));
        }
        let commit = Commit {
            has_custom_merge_id: options.id.is_some(),
            id: options.id.unwrap_or_else(|| self.generated_id()),
            seq: self.commits.len(),
            kind: CommitKind::Merge,
            symbol: options.kind.unwrap_or(CommitKind::Merge),
            tags: options.tags,
            parents: vec![current_head, other_head],
            branch: self.current_branch.clone(),
        };
        self.push_commit(commit);
        Ok(())
    }

    fn cherry_pick(&mut self, options: Options) -> Result<(), DiagramError> {
        let error = |reason: &str| DiagramError::Syntax(format!("cherry-pick: {reason}"));
        let source_id = options.id.ok_or_else(|| error("원본 id 없음"))?;
        let source = self
            .commits
            .iter()
            .find(|commit| commit.id == source_id)
            .ok_or_else(|| error("없는 원본 커밋"))?;
        if source.kind == CommitKind::Merge && options.parent.is_none() {
            return Err(error("병합 커밋은 parent가 필요함"));
        }
        if source.branch == self.current_branch {
            return Err(error("원본이 이미 현재 브랜치에 있음"));
        }
        let head = self
            .head
            .clone()
            .ok_or_else(|| error("현재 브랜치에 커밋 없음"))?;
        let default_tag = match &options.parent {
            Some(parent) if source.kind == CommitKind::Merge => {
                format!("cherry-pick:{source_id}|parent:{parent}")
            }
            _ => format!("cherry-pick:{source_id}"),
        };
        let has_tags = !options.tags.is_empty();
        let tags = if has_tags {
            options.tags
        } else {
            vec![default_tag]
        };
        let commit = Commit {
            id: self.generated_id(),
            seq: self.commits.len(),
            kind: CommitKind::CherryPick,
            symbol: CommitKind::CherryPick,
            has_custom_merge_id: false,
            tags: tags.into_iter().filter(|tag| !tag.is_empty()).collect(),
            parents: vec![head, source_id],
            branch: self.current_branch.clone(),
        };
        self.push_commit(commit);
        Ok(())
    }

    fn finish(self, direction: Direction, title: String, options: GitGraphOptions) -> GitGraph {
        let mut branches: Vec<(String, f64)> = self
            .branch_heads
            .into_iter()
            .map(|(name, _)| {
                let order = self.branch_orders.get(&name).copied().unwrap_or(0.0);
                (name, order)
            })
            .collect();
        branches.sort_by(|left, right| left.1.total_cmp(&right.1));
        GitGraph {
            direction,
            title,
            commits: self.commits,
            branches: branches.into_iter().map(|(name, _)| name).collect(),
            options,
        }
    }
}

fn name_argument(rest: &[Token]) -> Result<&str, DiagramError> {
    rest.first()
        .map(Token::text)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| DiagramError::Syntax("gitGraph 브랜치 이름 없음".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &str = "gitGraph\n commit\n branch develop\n checkout develop\n commit id: \"D1\"\n checkout main\n merge develop tag: \"v1\"";

    #[test]
    fn 브랜치와_병합을_기록한다() {
        let graph = parse(BASIC).unwrap();
        assert_eq!(graph.branches, vec!["main", "develop"]);
        assert_eq!(graph.commits.len(), 3);
        let merge = &graph.commits[2];
        assert_eq!(merge.kind, CommitKind::Merge);
        assert_eq!(
            merge.parents,
            vec![graph.commits[0].id.clone(), "D1".to_string()]
        );
        assert_eq!(merge.tags, vec!["v1"]);
        assert_eq!(merge.branch, "main");
    }

    #[test]
    fn 이름_없는_커밋_id는_정의마다_같다() {
        let first = parse(BASIC).unwrap();
        let second = parse(BASIC).unwrap();
        assert_eq!(first.commits[0].id, second.commits[0].id);
        assert!(first.commits[0].id.starts_with("0-"));
        assert_eq!(first.commits[0].id.len(), 9);
    }

    #[test]
    fn 브랜치_순서와_방향과_커밋_종류를_읽는다() {
        let graph = parse(
            "gitGraph TB:\n commit type: HIGHLIGHT \"msg\"\n branch b order: 2\n branch a order: 1\n commit id:\"X\" type: REVERSE",
        )
        .unwrap();
        assert_eq!(graph.direction, Direction::TopToBottom);
        assert_eq!(graph.branches, vec!["main", "a", "b"]);
        assert_eq!(graph.commits[0].kind, CommitKind::Highlight);
        assert_eq!(graph.commits[1].id, "X");
        assert_eq!(graph.commits[1].symbol, CommitKind::Reverse);
    }

    #[test]
    fn 체리픽은_원본을_두_번째_부모로_두고_태그를_단다() {
        let graph = parse(
            "gitGraph\n commit id: \"A\"\n branch dev\n commit id: \"B\"\n checkout main\n cherry-pick id: \"B\"",
        )
        .unwrap();
        let picked = &graph.commits[2];
        assert_eq!(picked.kind, CommitKind::CherryPick);
        assert_eq!(picked.parents, vec!["A", "B"]);
        assert_eq!(picked.tags, vec!["cherry-pick:B"]);
    }

    #[test]
    fn 잘못된_병합은_오류다() {
        assert!(parse("gitGraph\n commit\n merge main").is_err());
        assert!(parse("gitGraph\n commit\n checkout nowhere").is_err());
    }
}
