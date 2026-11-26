//! 정의의 `themeCSS`(데스크톱 경로만).
//!
//! Mermaid는 `themeCSS`를 자기 styles 뒤에 붙이고 모든 선택자 앞에 SVG id를 붙인다(`#id .node rect`). 그래서 Mermaid가 쓰는
//! 선택자와 똑같은 선택자로 쓴 규칙은 특이도가 같고 뒤에 와서 테마 색을 이긴다. 네이티브 장면에는 CSS 클래스가 없으므로,
//! 다이어그램이 자기 Mermaid 선택자 이름으로 [`ThemeCss::color`]를 물어 맞는 선언만 쓴다. 선택자는 공백만 정리해 글자 그대로
//! 견주고(후손·자식 결합자 해석, 특이도 계산, 상속은 하지 않는다), `@` 규칙과 규칙 안에 중첩된 규칙은 버린다.
//! 같은 선택자의 선언은 뒤의 것이 이기고 `!important`는 그렇지 않은 선언을 이긴다.

use crate::scene::Color;
use crate::theme_variables::parse_theme_color;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeCss {
    rules: Vec<CssRule>,
}

#[derive(Clone, Debug, PartialEq)]
struct CssRule {
    selectors: Vec<String>,
    declarations: Vec<CssDeclaration>,
}

#[derive(Clone, Debug, PartialEq)]
struct CssDeclaration {
    property: String,
    value: String,
    is_important: bool,
}

impl ThemeCss {
    pub fn parse(text: &str) -> Self {
        let text = strip_comments(text);
        let mut rules = Vec::new();
        let mut rest = text.as_str();
        while let Some(open) = rest.find('{') {
            let prelude = rest[..open].trim();
            let (body, after) = split_block(&rest[open + 1..]);
            rest = after;
            if prelude.starts_with('@') {
                continue;
            }
            rules.push(CssRule {
                selectors: prelude.split(',').map(normalize_selector).collect(),
                declarations: parse_declarations(body),
            });
        }
        Self { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// `selectors` 가운데 하나와 같은 선택자로 쓴 `property` 선언의 값
    pub fn value(&self, selectors: &[&str], property: &str) -> Option<&str> {
        let mut chosen: Option<&CssDeclaration> = None;
        for rule in &self.rules {
            if !rule
                .selectors
                .iter()
                .any(|selector| selectors.contains(&selector.as_str()))
            {
                continue;
            }
            for declaration in rule
                .declarations
                .iter()
                .filter(|declaration| declaration.property == property)
            {
                if chosen.is_none_or(|current| declaration.is_important || !current.is_important) {
                    chosen = Some(declaration);
                }
            }
        }
        chosen.map(|declaration| declaration.value.as_str())
    }

    /// 브라우저가 읽는 색 값만
    pub fn color(&self, selectors: &[&str], property: &str) -> Option<Color> {
        parse_theme_color(self.value(selectors, property)?)
    }
}

fn strip_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        result.push_str(&rest[..start]);
        rest = match rest[start + 2..].find("*/") {
            Some(end) => &rest[start + 2 + end + 2..],
            None => "",
        };
    }
    result.push_str(rest);
    result
}

/// 여는 중괄호 다음부터 짝이 맞는 닫는 중괄호까지(안쪽 블록은 본문에서 빠진다)와 그 뒤
fn split_block(text: &str) -> (String, &str) {
    let mut depth = 0usize;
    let mut body = String::new();
    for (index, character) in text.char_indices() {
        match character {
            '{' => depth += 1,
            '}' if depth == 0 => return (body, &text[index + 1..]),
            '}' => depth -= 1,
            _ if depth == 0 => body.push(character),
            _ => {}
        }
    }
    (body, "")
}

fn normalize_selector(selector: &str) -> String {
    selector.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_declarations(body: String) -> Vec<CssDeclaration> {
    body.split(';')
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let value = value.trim();
            let (value, is_important) = match value.strip_suffix("!important") {
                Some(value) => (value.trim(), true),
                None => (value, false),
            };
            Some(CssDeclaration {
                property: property.trim().to_ascii_lowercase(),
                value: value.to_string(),
                is_important,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 같은_선택자의_뒤_선언과_important가_이긴다() {
        let css = ThemeCss::parse(
            "/* note */ .node rect, .node  polygon { fill: #f00 !important; stroke: blue } @media print { .node rect { fill: green } } .node rect { fill: #0f0; stroke: red; .inner { fill: black } }",
        );
        assert_eq!(css.value(&[".node rect"], "fill"), Some("#f00"));
        assert_eq!(css.value(&[".node polygon"], "stroke"), Some("blue"));
        assert_eq!(
            css.color(&[".node rect"], "stroke"),
            Some(Color::rgb(255, 0, 0))
        );
        assert_eq!(css.value(&["rect"], "fill"), None);
    }
}
