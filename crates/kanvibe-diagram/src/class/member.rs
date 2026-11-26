//! classTypes.ts의 ClassMember: 멤버 한 줄을 화면에 쓰는 글자와 표시(정적·추상)로 바꾼다.
//!
//! 접근 제한자(`+ - # ~`)는 앞에 그대로 두고, `~T~` 제네릭은 `<T>`로 바꾼다. 메서드는 `이름(인자) : 반환형`
//! 꼴이 된다. 끝의 `$`(정적, 밑줄)·`*`(추상, 기울임) 표시는 글자에서 빼고 [`Classifier`]로 남긴다.

const VISIBILITY_MARKS: [char; 4] = ['#', '+', '~', '-'];

/// parseClassifier: `*`는 `font-style: italic`, `$`는 `text-decoration: underline`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classifier {
    Abstract,
    Static,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassMember {
    pub text: String,
    pub classifier: Option<Classifier>,
}

fn classifier_of(mark: char) -> Option<Classifier> {
    match mark {
        '*' => Some(Classifier::Abstract),
        '$' => Some(Classifier::Static),
        _ => None,
    }
}

/// 멤버 줄이 메서드인지: `)`가 첫 글자가 아닌 자리에 있으면 메서드다(addMember)
pub fn is_method(member: &str) -> bool {
    member.find(')').is_some_and(|index| index > 0)
}

/// 속성 한 줄: 마지막 글자가 `$`·`*`면 표시다
pub fn attribute_member(input: &str) -> ClassMember {
    let (visibility, rest) = split_visibility(input);
    let classifier = rest.chars().last().and_then(classifier_of);
    let body = match classifier {
        Some(_) => &rest[..rest.len() - 1],
        None => rest,
    };
    ClassMember {
        text: format!(
            "{visibility}{}",
            parse_generic_types(&keep_leading_space(body))
        ),
        classifier,
    }
}

/// 메서드 한 줄. 원본 정규식 `([#+~-])?(.+)\((.*)\)([\s$*])?(.*)([$*])?`을 따른다
pub fn method_member(input: &str) -> ClassMember {
    let (visibility, rest) = split_visibility(input);
    let Some((open, close)) = method_parentheses(rest) else {
        return ClassMember {
            text: format!("{visibility}{rest}"),
            classifier: None,
        };
    };
    let name = keep_leading_space(&rest[..open]);
    let parameters = rest[open + 1..close].trim();
    let (return_type, classifier) = method_return_type(&rest[close + 1..]);
    let mut text = format!(
        "{visibility}{}({})",
        parse_generic_types(&name),
        parse_generic_types(parameters)
    );
    if !return_type.is_empty() {
        text.push_str(" : ");
        text.push_str(&parse_generic_types(&return_type));
    }
    ClassMember { text, classifier }
}

fn split_visibility(input: &str) -> (String, &str) {
    match input.chars().next() {
        Some(first) if VISIBILITY_MARKS.contains(&first) => {
            (first.to_string(), &input[first.len_utf8()..])
        }
        _ => (String::new(), input),
    }
}

/// `this.id.startsWith(" ") ? " " + this.id.trim() : this.id.trim()`
fn keep_leading_space(name: &str) -> String {
    if name.starts_with(' ') {
        format!(" {}", name.trim())
    } else {
        name.trim().to_string()
    }
}

/// 탐욕적인 `(.+)\((.*)\)`: 뒤에 `)`가 있는 마지막 `(`와 그 뒤 마지막 `)`
fn method_parentheses(rest: &str) -> Option<(usize, usize)> {
    let close = rest.rfind(')')?;
    let open = rest[..close].rfind('(').filter(|open| *open > 0)?;
    Some((open, close))
}

/// 닫는 괄호 뒤: 바로 붙은 `$`·`*`가 표시이고, 공백이거나 없으면 반환형 끝의 `$`·`*`가 표시다
fn method_return_type(tail: &str) -> (String, Option<Classifier>) {
    let mut characters = tail.chars();
    let first = characters.next();
    let direct = first.and_then(classifier_of);
    let return_type = if direct.is_some() || first.is_some_and(char::is_whitespace) {
        characters.as_str().trim()
    } else {
        tail.trim()
    };
    if direct.is_some() {
        return (return_type.to_string(), direct);
    }
    match return_type.chars().last().and_then(classifier_of) {
        Some(classifier) => (
            return_type[..return_type.len() - 1].to_string(),
            Some(classifier),
        ),
        None => (return_type.to_string(), None),
    }
}

/// common.parseGenericTypes: 쉼표로 나눈 조각마다 짝지은 `~`를 `<`·`>`로 바꾼다
pub fn parse_generic_types(input: &str) -> String {
    let sets: Vec<&str> = split_keep_commas(input);
    let mut output: Vec<String> = Vec::new();
    let mut index = 0;
    while index < sets.len() {
        let mut current = sets[index].to_string();
        let is_joinable = sets[index] == "," && index > 0 && index + 1 < sets.len();
        if is_joinable && should_combine_sets(sets[index - 1], sets[index + 1]) {
            current = format!("{},{}", sets[index - 1], sets[index + 1]);
            index += 1;
            output.pop();
        }
        output.push(process_set(&current));
        index += 1;
    }
    output.concat()
}

/// JS `split(/(,)/)`: 쉼표도 조각으로 남긴다
fn split_keep_commas(input: &str) -> Vec<&str> {
    let mut sets = Vec::new();
    let mut start = 0;
    for (index, _) in input.match_indices(',') {
        sets.push(&input[start..index]);
        sets.push(",");
        start = index + 1;
    }
    sets.push(&input[start..]);
    sets
}

fn should_combine_sets(previous: &str, next: &str) -> bool {
    previous.matches('~').count() == 1 && next.matches('~').count() == 1
}

fn process_set(input: &str) -> String {
    let tilde_count = input.matches('~').count();
    if tilde_count <= 1 {
        return input.to_string();
    }
    let has_starting_tilde = !tilde_count.is_multiple_of(2) && input.starts_with('~');
    let body = if has_starting_tilde {
        &input[1..]
    } else {
        input
    };
    let mut characters: Vec<char> = body.chars().collect();
    while let (Some(first), Some(last)) = (
        characters.iter().position(|character| *character == '~'),
        characters.iter().rposition(|character| *character == '~'),
    ) {
        if first == last {
            break;
        }
        characters[first] = '<';
        characters[last] = '>';
    }
    let text: String = characters.into_iter().collect();
    if has_starting_tilde {
        format!("~{text}")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 메서드는_반환형을_콜론으로_잇는다() {
        let text = |input: &str| method_member(input).text;
        assert_eq!(text("+isMammal() bool"), "+isMammal() : bool");
        assert_eq!(text("+mate()"), "+mate()");
        assert_eq!(
            text("+render(request) DiagramImage"),
            "+render(request) : DiagramImage"
        );
        let static_method = method_member("+count()$ int");
        assert_eq!(static_method.text, "+count() : int");
        assert_eq!(static_method.classifier, Some(Classifier::Static));
        let abstract_method = method_member("-load() int*");
        assert_eq!(abstract_method.text, "-load() : int");
        assert_eq!(abstract_method.classifier, Some(Classifier::Abstract));
    }

    #[test]
    fn 속성은_표시를_빼고_제네릭을_바꾼다() {
        assert_eq!(attribute_member("+List~T~ items").text, "+List<T> items");
        assert_eq!(attribute_member("-int sizeInFeet").classifier, None);
        let static_field = attribute_member("+String name$");
        assert_eq!(static_field.text, "+String name");
        assert_eq!(static_field.classifier, Some(Classifier::Static));
    }

    #[test]
    fn 겹친_제네릭과_쉼표() {
        assert_eq!(parse_generic_types("List~List~int~~"), "List<List<int>>");
        assert_eq!(parse_generic_types("Map~K,V~"), "Map<K,V>");
    }
}
