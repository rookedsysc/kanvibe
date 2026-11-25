//! Mermaid `sanitizeText()`(DOMPurify)가 글자에 남기는 모양.
//!
//! DOMPurify는 글자를 HTML로 읽었다가 다시 직렬화한다. 그 결과를 SVG `<text>`에 `textContent`로 넣는
//! 다이어그램(gitGraph, quadrantChart 등)은 직렬화된 모양이 그대로 보인다. 맨 `&`는 `&amp;`, 태그가 아닌
//! `<`·`>`는 `&lt;`·`&gt;`, 줄바꿈 태그 `<br/>`·`<br />`는 `<br>`가 되고, 나머지 태그는 그대로 남긴다.

/// DOMPurify를 거친 글자
pub fn sanitize_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let consumed = match character {
            '&' => push_ampersand(&mut output, rest),
            '<' => push_angle(&mut output, rest),
            '>' => {
                output.push_str("&gt;");
                1
            }
            '\u{A0}' => {
                output.push_str("&nbsp;");
                character.len_utf8()
            }
            _ => {
                output.push(character);
                character.len_utf8()
            }
        };
        rest = &rest[consumed..];
    }
    output
}

/// `&이름;`·`&#숫자;` 꼴 참조는 두고, 맨 `&`는 `&amp;`로 바꾼다
fn push_ampersand(output: &mut String, rest: &str) -> usize {
    let reference_length = rest[1..].find(';').filter(|end| {
        *end > 0
            && rest[1..1 + end]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '#')
    });
    match reference_length {
        Some(end) => {
            output.push_str(&rest[..end + 2]);
            end + 2
        }
        None => {
            output.push_str("&amp;");
            1
        }
    }
}

/// 태그(`<글자`·`</글자`)는 `>`까지 옮기고 줄바꿈 태그는 `<br>`로 맞춘다. 태그가 아니면 `&lt;`
fn push_angle(output: &mut String, rest: &str) -> usize {
    let starts_tag = rest[1..]
        .trim_start_matches('/')
        .starts_with(|c: char| c.is_ascii_alphabetic());
    let Some(end) = rest.find('>').filter(|_| starts_tag) else {
        output.push_str("&lt;");
        return 1;
    };
    let tag = &rest[..=end];
    let name = tag[1..tag.len() - 1].trim_end_matches('/').trim();
    if name.eq_ignore_ascii_case("br") {
        output.push_str("<br>");
    } else {
        output.push_str(tag);
    }
    end + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 맨_앰퍼샌드와_꺾쇠를_엔티티로_바꾼다() {
        assert_eq!(sanitize_text("R&D <-> x"), "R&amp;D &lt;-&gt; x");
        assert_eq!(sanitize_text("a &amp; b"), "a &amp; b");
    }

    #[test]
    fn 줄바꿈_태그는_br로_맞춘다() {
        assert_eq!(
            sanitize_text("a<br/>b<BR />c<b>d</b>"),
            "a<br>b<br>c<b>d</b>"
        );
    }
}
