//! marked GFM 자동 링크 규칙(`inline.gfm.url`과 `_backpedal`)을 정규식 없이 옮겼다.
//!
//! - 주소: `http://`·`https://`·`ftp://`(대소문자 무시) 또는 `www.` 뒤에 ASCII 글자·숫자·`-`가 와야 하고,
//!   공백이나 `<` 앞까지 이어진다. 그 뒤 marked처럼 끝 문장부호·짝 없는 괄호·끝 엔티티를 더 줄지 않을 때까지 뺀다.
//! - 이메일: `[A-Za-z0-9._+-]+@` 뒤에 점으로 나뉜 도메인이 오고, 바로 뒤가 `-`·`_`면 안 된다. 정규식 되짚기처럼
//!   도메인 마디를 줄여 가며 처음 맞는 끝을 고른다.

/// 자동 링크 한 개(원문 그대로의 글자와 marked가 만든 주소)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Autolink {
    pub text: String,
    pub href: String,
}

fn is_js_whitespace(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

fn strip_protocol(rest: &str) -> Option<&str> {
    let lower: String = rest
        .chars()
        .take(8)
        .collect::<String>()
        .to_ascii_lowercase();
    ["https://", "http://", "ftp://"]
        .iter()
        .find(|protocol| lower.starts_with(*protocol))
        .map(|protocol| &rest[protocol.len()..])
}

/// `_backpedal` 한 번: 정규식처럼 앞에서부터 욕심껏 먹고 더 먹을 수 없으면 멈춘다
fn backpedal_once(url: &str) -> &str {
    let bytes = url.as_bytes();
    let is_trailing_punctuation = |byte: u8| b"?!.,:;*_'\"~)".contains(&byte);
    let is_plain = |byte: u8| !b"?!.,:;*_'\"~()&".contains(&byte);
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if is_plain(byte) {
            index += 1;
        } else if byte == b'(' {
            let Some(close) = url[index..].find(')') else {
                break;
            };
            index += close + 1;
        } else if byte == b'&' {
            if is_entity_at_end(&url[index + 1..]) {
                break;
            }
            index += 1;
        } else {
            let run = bytes[index..]
                .iter()
                .take_while(|byte| is_trailing_punctuation(**byte))
                .count();
            let end = index + run;
            if end < bytes.len() {
                index = end;
            } else if run > 1 {
                index = end - 1;
            } else {
                break;
            }
        }
    }
    &url[..index]
}

/// `&(?![a-zA-Z0-9]+;$)`의 부정 조건: 끝까지가 `이름;`이다
fn is_entity_at_end(rest: &str) -> bool {
    rest.strip_suffix(';').is_some_and(|name| {
        !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

/// 주소 자동 링크. 맞으면 링크가 차지하는 원문 길이와 링크를 돌려준다
pub fn match_url(rest: &str) -> Option<(usize, Autolink)> {
    let (after_prefix, is_www) = match strip_protocol(rest) {
        Some(after) => (after, false),
        None => (rest.strip_prefix("www.")?, true),
    };
    let first = after_prefix.chars().next()?;
    if !(first.is_ascii_alphanumeric() || first == '-') {
        return None;
    }
    let end = rest
        .find(|character: char| is_js_whitespace(character) || character == '<')
        .unwrap_or(rest.len());
    let mut url = &rest[..end];
    loop {
        let shorter = backpedal_once(url);
        if shorter == url {
            break;
        }
        url = shorter;
    }
    if url.is_empty() {
        return None;
    }
    let href = if is_www {
        format!("http://{url}")
    } else {
        url.to_string()
    };
    Some((
        url.len(),
        Autolink {
            text: url.to_string(),
            href,
        },
    ))
}

fn is_domain_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// `(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![-_])`를 되짚기 순서대로 맞춰 끝 위치를 찾는다
fn domain_groups_end(bytes: &[u8], position: usize, has_group: bool) -> Option<usize> {
    if bytes.get(position) == Some(&b'.') {
        let run_end = position
            + 1
            + bytes[position + 1..]
                .iter()
                .take_while(|byte| is_domain_byte(**byte))
                .count();
        for end in (position + 2..=run_end).rev() {
            if bytes[end - 1].is_ascii_alphanumeric()
                && let Some(found) = domain_groups_end(bytes, end, true)
            {
                return Some(found);
            }
        }
    }
    let is_followed_by_dash = matches!(bytes.get(position), Some(b'-' | b'_'));
    (has_group && !is_followed_by_dash).then_some(position)
}

/// 이메일 자동 링크. marked의 글자 규칙은 `@` 앞 이메일 글자 줄의 모든 자리에서 이 규칙을 시도한다
pub fn match_email(rest: &str) -> Option<(usize, Autolink)> {
    let bytes = rest.as_bytes();
    let local = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(byte))
        .count();
    if local == 0 || bytes.get(local) != Some(&b'@') {
        return None;
    }
    let label_start = local + 1;
    let label = bytes[label_start..]
        .iter()
        .take_while(|byte| is_domain_byte(**byte))
        .count();
    if label == 0 {
        return None;
    }
    let end = domain_groups_end(bytes, label_start + label, false)?;
    let text = rest[..end].to_string();
    Some((
        end,
        Autolink {
            href: format!("mailto:{text}"),
            text,
        },
    ))
}

/// marked 글자 규칙이 이메일을 시도하는 자리: 여기부터 `@`까지 이메일 글자만 이어진다
pub fn is_email_attempt_position(rest: &str) -> bool {
    let run = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+/=?_`{|}~-".contains(byte))
        .count();
    run > 0 && rest.as_bytes().get(run) == Some(&b'@')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url_text(source: &str) -> Option<String> {
        match_url(source).map(|(_, link)| link.text)
    }

    fn email_text(source: &str) -> Option<String> {
        match_email(source).map(|(_, link)| link.text)
    }

    #[test]
    fn 주소_끝의_문장부호와_짝_없는_괄호를_뺀다() {
        assert_eq!(
            url_text("https://example.com/*foo*").as_deref(),
            Some("https://example.com/*foo")
        );
        assert_eq!(
            url_text("https://a.com).").as_deref(),
            Some("https://a.com")
        );
        assert_eq!(
            url_text("https://a.com/(x)").as_deref(),
            Some("https://a.com/(x)")
        );
        assert_eq!(
            url_text("https://a.com/(x").as_deref(),
            Some("https://a.com/")
        );
        assert_eq!(
            url_text("https://x.com&amp;").as_deref(),
            Some("https://x.com")
        );
        assert_eq!(url_text("http://a").as_deref(), Some("http://a"));
        assert_eq!(
            match_url("www.google")
                .map(|(_, link)| link.href)
                .as_deref(),
            Some("http://www.google")
        );
    }

    #[test]
    fn 주소는_ascii_도메인으로_시작해야_한다() {
        assert_eq!(url_text("https://예시.com/경로"), None);
        assert_eq!(url_text("https://"), None);
        assert_eq!(url_text("wwwx.com"), None);
    }

    #[test]
    fn 이메일은_도메인_마디를_되짚어_끝을_고른다() {
        assert_eq!(
            email_text("a.b-c+d@ex-ample.co.kr").as_deref(),
            Some("a.b-c+d@ex-ample.co.kr")
        );
        assert_eq!(email_text("a@b"), None);
        assert_eq!(email_text("x@b.c_"), None);
        assert_eq!(email_text("a@b.c-"), None);
        assert_eq!(email_text("a@b.c.").as_deref(), Some("a@b.c"));
        assert_eq!(email_text("x@y.ab_").as_deref(), Some("x@y.a"));
    }
}
