//! ishikawa.jison 문법과 IshikawaDB.addNode: 머리줄(`ishikawa`·`ishikawa-beta`) 다음 첫 줄이 결과(머리),
//! 그다음 줄들은 들여쓰기 깊이로 원인 나무를 만든다. 두 번째 줄의 들여쓰기가 1단계 기준이고, 그보다 얕은
//! 줄도 1단계로 붙는다. `%%` 주석과 빈 줄은 건너뛴다.

use crate::DiagramError;
use crate::sanitize::sanitize_text;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CauseNode {
    pub text: String,
    pub children: Vec<CauseNode>,
}

pub fn parse(definition: &str) -> Result<CauseNode, DiagramError> {
    let mut lines = definition
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("%%"));
    let header = lines.next().unwrap_or("").trim().to_ascii_lowercase();
    if header != "ishikawa" && header != "ishikawa-beta" {
        return Err(DiagramError::Syntax("ishikawa 머리줄".to_string()));
    }
    let entries: Vec<(usize, String)> = lines
        .map(|line| {
            let text = line.trim_start();
            (line.len() - text.len(), sanitize_text(text.trim()))
        })
        .collect();
    let Some(((_, head), causes)) = entries.split_first() else {
        return Err(DiagramError::Syntax(
            "ishikawa 결과 줄이 없습니다".to_string(),
        ));
    };
    let base_level = causes.first().map_or(0, |(level, _)| *level);
    let leveled: Vec<(usize, String)> = causes
        .iter()
        .map(|(raw, text)| ((*raw + 1).saturating_sub(base_level).max(1), text.clone()))
        .collect();
    let mut position = 0;
    Ok(CauseNode {
        text: head.clone(),
        children: build_children(&leveled, &mut position, 0),
    })
}

/// 스택으로 부모를 찾는 addNode를 재귀로 옮겼다: 깊이가 `parent_level`보다 큰 줄만 이 부모 아래에 붙는다
fn build_children(
    entries: &[(usize, String)],
    position: &mut usize,
    parent_level: usize,
) -> Vec<CauseNode> {
    let mut children = Vec::new();
    while let Some((level, text)) = entries.get(*position) {
        if *level <= parent_level {
            break;
        }
        *position += 1;
        let grandchildren = build_children(entries, position, *level);
        children.push(CauseNode {
            text: text.clone(),
            children: grandchildren,
        });
    }
    children
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 들여쓰기로_원인_나무를_만든다() {
        let root = parse("ishikawa-beta\n  Effect\n  A\n    a1\n      a11\n    a2\n  B\n").unwrap();
        assert_eq!(root.text, "Effect");
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].children.len(), 2);
        assert_eq!(root.children[0].children[0].children[0].text, "a11");
        assert!(root.children[1].children.is_empty());
    }

    #[test]
    fn 첫_원인보다_얕은_줄도_첫_단계로_붙는다() {
        let root = parse("ishikawa\nEffect\n    A\n  B\n%% 주석\n      b1\n").unwrap();
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[1].children[0].text, "b1");
    }

    #[test]
    fn 머리줄이_다르면_문법_오류다() {
        assert!(parse("fishbone\n  Effect").is_err());
    }
}
