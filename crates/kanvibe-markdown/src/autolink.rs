//! marked GFM 자동 링크(`https://`, `http://`, `www.`). 끝의 문장부호와 짝 없는 `)`는 링크에서 뺀다.

use crate::model::Inline;

fn find_start(text: &str) -> Option<usize> {
    let candidates = ["https://", "http://", "www."];
    candidates
        .iter()
        .filter_map(|prefix| {
            text.match_indices(prefix)
                .map(|(index, _)| index)
                .find(|index| {
                    *index == 0
                        || !text[..*index]
                            .chars()
                            .next_back()
                            .is_some_and(char::is_alphanumeric)
                })
        })
        .min()
}

fn trim_trailing(candidate: &str) -> &str {
    let mut end = candidate.len();
    loop {
        let current = &candidate[..end];
        let Some(last) = current.chars().next_back() else {
            return current;
        };
        let is_unbalanced_paren =
            last == ')' && current.matches('(').count() < current.matches(')').count();
        if matches!(
            last,
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"'
        ) || is_unbalanced_paren
        {
            end -= last.len_utf8();
        } else {
            return current;
        }
    }
}

/// 글자 안의 URL을 링크로 바꾼다
pub fn link_urls(text: &str) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut rest = text;
    while let Some(start) = find_start(rest) {
        let tail = &rest[start..];
        let end = tail
            .find(|character: char| character.is_whitespace() || character == '<')
            .unwrap_or(tail.len());
        let url = trim_trailing(&tail[..end]);
        let has_domain = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .contains('.')
            || !url.starts_with("www.");
        if url.len() <= "https://".len() || !has_domain {
            result.push(Inline::Text(rest[..start + 1].to_string()));
            rest = &rest[start + 1..];
            continue;
        }
        if start > 0 {
            result.push(Inline::Text(rest[..start].to_string()));
        }
        let href = if url.starts_with("www.") {
            format!("http://{url}")
        } else {
            url.to_string()
        };
        result.push(Inline::Link {
            href,
            title: None,
            content: vec![Inline::Text(url.to_string())],
        });
        rest = &rest[start + url.len()..];
    }
    if !rest.is_empty() {
        result.push(Inline::Text(rest.to_string()));
    }
    let mut merged: Vec<Inline> = Vec::new();
    for inline in result {
        match (merged.last_mut(), inline) {
            (Some(Inline::Text(previous)), Inline::Text(next)) => previous.push_str(&next),
            (_, inline) => merged.push(inline),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 문장부호를_뺀_url을_링크로_바꾼다() {
        let inlines = link_urls("see https://example.com/a(b). and www.kanvibe.dev!");
        let hrefs: Vec<&str> = inlines
            .iter()
            .filter_map(|inline| match inline {
                Inline::Link { href, .. } => Some(href.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            hrefs,
            ["https://example.com/a(b)", "http://www.kanvibe.dev"]
        );
    }
}
