//! marked가 만든 HTML을 브라우저처럼 파싱(html5ever)하고 DOMPurify 3.4.2(`USE_PROFILES: { html, mathMl }`,
//! `FORBID_TAGS: style`, `FORBID_ATTR: style`)가 남기는 요소만 남긴다.
//!
//! - 허용 목록 밖 요소는 태그만 지우고 내용은 남긴다. 다만 `FORBID_CONTENTS`(script·style·svg·iframe·template …)는
//!   내용까지 지운다. HTML 이름공간의 MathML 이름 요소처럼 이름공간이 맞지 않는 요소도 내용까지 지운다.
//! - 속성은 모델이 읽는 것만 남기되, DOMPurify처럼 앞뒤 공백을 자르고 주소 검사(`ALLOWED_URI_REGEXP`,
//!   `img` 등의 `data:`)를 통과한 값만 남긴다. `on*`·`style`은 모델이 읽지 않으므로 함께 사라진다.
//! - 주석·처리 명령은 지운다.

use html5ever::tendril::TendrilSink;
use html5ever::{
    LocalName, Namespace, ParseOpts, QualName, local_name, namespace_url, ns, parse_fragment,
};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// 정화한 요소 나무의 마디
#[derive(Clone, Debug, PartialEq)]
pub enum HtmlNode {
    Element(HtmlElement),
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct HtmlElement {
    /// 소문자 지역 이름
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<HtmlNode>,
}

impl HtmlElement {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

const HTML_TAGS: &[&str] = &[
    "a",
    "abbr",
    "acronym",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "bdi",
    "bdo",
    "big",
    "blink",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "content",
    "data",
    "datalist",
    "dd",
    "decorator",
    "del",
    "details",
    "dfn",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "element",
    "em",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "main",
    "map",
    "mark",
    "marquee",
    "menu",
    "menuitem",
    "meter",
    "nav",
    "nobr",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "search",
    "section",
    "select",
    "shadow",
    "slot",
    "small",
    "source",
    "spacer",
    "span",
    "strike",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

const MATHML_TAGS: &[&str] = &[
    "math",
    "menclose",
    "merror",
    "mfenced",
    "mfrac",
    "mglyph",
    "mi",
    "mlabeledtr",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msup",
    "msubsup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "mprescripts",
];

/// 허용 목록에는 없지만 이름공간 검사에서 MathML 이름으로 보는 요소
const MATHML_DISALLOWED_TAGS: &[&str] = &[
    "maction",
    "maligngroup",
    "malignmark",
    "mlongdiv",
    "mscarries",
    "mscarry",
    "msgroup",
    "mstack",
    "msline",
    "msrow",
    "semantics",
    "annotation",
    "annotation-xml",
    "none",
];

const FORBID_CONTENTS: &[&str] = &[
    "annotation-xml",
    "audio",
    "colgroup",
    "desc",
    "foreignobject",
    "head",
    "iframe",
    "math",
    "mi",
    "mn",
    "mo",
    "ms",
    "mtext",
    "noembed",
    "noframes",
    "noscript",
    "plaintext",
    "script",
    "style",
    "svg",
    "template",
    "thead",
    "title",
    "video",
    "xmp",
];

/// 모델이 읽는 속성(모두 DOMPurify html 속성 허용 목록에 있다)
const MODEL_ATTRIBUTES: &[&str] = &[
    "href", "title", "src", "alt", "width", "height", "start", "type", "checked", "open", "align",
    "colspan", "rowspan", "class", "hidden",
];

/// DOMPurify `URI_SAFE_ATTRIBUTES` 중 모델이 읽는 것
const URI_SAFE_ATTRIBUTES: &[&str] = &["alt", "class", "title"];

const DATA_URI_TAGS: &[&str] = &["audio", "video", "img", "source", "image", "track"];

enum Verdict {
    Keep,
    /// 태그만 지우고 내용은 부모 자리에 남긴다
    Unwrap,
    Remove,
}

/// HTML 조각을 `<body>` 문맥으로 파싱해 정화한 마디 목록을 돌려준다
pub fn sanitize(html: &str) -> Vec<HtmlNode> {
    let context = QualName::new(None, ns!(html), local_name!("body"));
    let mut options = ParseOpts::default();
    options.tree_builder.scripting_enabled = false;
    let dom = parse_fragment(RcDom::default(), options, context, Vec::new()).one(html);
    let document_children = dom.document.children.borrow();
    let Some(root) = document_children.first() else {
        return Vec::new();
    };
    sanitize_children(root, &ns!(html))
}

fn sanitize_children(parent: &Handle, parent_namespace: &Namespace) -> Vec<HtmlNode> {
    let mut nodes = Vec::new();
    for child in parent.children.borrow().iter() {
        sanitize_node(child, parent_namespace, &mut nodes);
    }
    nodes
}

fn sanitize_node(node: &Handle, parent_namespace: &Namespace, output: &mut Vec<HtmlNode>) {
    let NodeData::Element { name, attrs, .. } = &node.data else {
        if let NodeData::Text { contents } = &node.data {
            output.push(HtmlNode::Text(contents.borrow().to_string()));
        }
        return;
    };
    match verdict(&name.ns, &name.local, parent_namespace) {
        Verdict::Keep => {
            let tag = name.local.to_string();
            let attributes = sanitize_attributes(&tag, &attrs.borrow());
            output.push(HtmlNode::Element(HtmlElement {
                children: sanitize_children(node, &name.ns),
                name: tag,
                attributes,
            }));
        }
        Verdict::Unwrap => output.extend(sanitize_children(node, parent_namespace)),
        Verdict::Remove => {}
    }
}

/// DOMPurify `_sanitizeElements`: 허용 검사 뒤 이름공간 검사(`_checkValidNamespace`)
fn verdict(namespace: &Namespace, local: &LocalName, parent_namespace: &Namespace) -> Verdict {
    let name: &str = local;
    let is_mathml_name = MATHML_TAGS.contains(&name) || MATHML_DISALLOWED_TAGS.contains(&name);
    let is_allowed = *namespace != ns!(svg)
        && name != "style"
        && (HTML_TAGS.contains(&name) || MATHML_TAGS.contains(&name));
    if !is_allowed {
        return if FORBID_CONTENTS.contains(&name) || *namespace == ns!(svg) {
            Verdict::Remove
        } else {
            Verdict::Unwrap
        };
    }
    let is_valid_namespace = if *namespace == ns!(mathml) {
        if *parent_namespace == ns!(html) {
            name == "math"
        } else {
            is_mathml_name
        }
    } else {
        !is_mathml_name
    };
    if is_valid_namespace {
        Verdict::Keep
    } else {
        Verdict::Remove
    }
}

fn is_js_whitespace(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

/// DOMPurify `ATTR_WHITESPACE`
fn is_attribute_whitespace(character: char) -> bool {
    matches!(character, '\u{0}'..='\u{20}' | '\u{a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'..='\u{2029}' | '\u{205f}' | '\u{3000}')
}

fn sanitize_attributes(tag: &str, attributes: &[html5ever::Attribute]) -> Vec<(String, String)> {
    attributes
        .iter()
        .filter(|attribute| attribute.name.ns == ns!())
        .filter_map(|attribute| {
            let name = attribute.name.local.to_string();
            let value = attribute.value.trim_matches(is_js_whitespace).to_string();
            let is_kept = MODEL_ATTRIBUTES.contains(&name.as_str())
                && is_valid_attribute_value(tag, &name, &value);
            is_kept.then_some((name, value))
        })
        .collect()
}

/// DOMPurify `_isValidAttribute`의 주소 검사와 `SAFE_FOR_XML` 속성 검사
fn is_valid_attribute_value(tag: &str, name: &str, value: &str) -> bool {
    let lower = value.to_lowercase();
    let has_markup_breakout = lower.contains("-->")
        || lower.contains("--!>")
        || lower.contains("]>")
        || ["</style", "</title", "</textarea"]
            .iter()
            .any(|closing| lower.contains(closing));
    if has_markup_breakout {
        return false;
    }
    if URI_SAFE_ATTRIBUTES.contains(&name) {
        return true;
    }
    let compact: String = value
        .chars()
        .filter(|character| !is_attribute_whitespace(*character))
        .collect();
    if is_allowed_uri(&compact) {
        return true;
    }
    let is_data_uri = matches!(name, "src" | "href")
        && value.starts_with("data:")
        && DATA_URI_TAGS.contains(&tag);
    is_data_uri || value.is_empty()
}

/// `/^(?:(?:(?:f|ht)tps?|mailto|tel|callto|sms|cid|xmpp|matrix):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i`
fn is_allowed_uri(value: &str) -> bool {
    const SCHEMES: [&str; 11] = [
        "ftp:", "ftps:", "http:", "https:", "mailto:", "tel:", "callto:", "sms:", "cid:", "xmpp:",
        "matrix:",
    ];
    let lower = value.to_ascii_lowercase();
    if SCHEMES.iter().any(|scheme| lower.starts_with(scheme)) {
        return true;
    }
    let Some(first) = lower.chars().next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return true;
    }
    let run = lower
        .bytes()
        .take_while(|byte| byte.is_ascii_lowercase() || matches!(byte, b'+' | b'.' | b'-'))
        .count();
    lower.as_bytes().get(run) != Some(&b':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 위험한_주소는_버린다() {
        assert!(!is_valid_attribute_value(
            "a",
            "href",
            "javascript:alert(1)"
        ));
        assert!(!is_valid_attribute_value(
            "a",
            "href",
            "jav\tascript:alert(1)"
        ));
        assert!(!is_valid_attribute_value("a", "href", "data:text/html,x"));
        assert!(is_valid_attribute_value(
            "img",
            "src",
            "data:image/png;base64,AAAA"
        ));
        assert!(is_valid_attribute_value("img", "src", "data:text/html,x"));
        assert!(!is_valid_attribute_value(
            "img",
            "src",
            "DATA:image/png;base64,AAAA"
        ));
        assert!(is_valid_attribute_value("a", "href", "https://example.com"));
        assert!(is_valid_attribute_value("a", "href", "/relative/path"));
        assert!(is_valid_attribute_value("a", "href", "docs/readme.md"));
        assert!(is_valid_attribute_value("a", "title", "javascript:x"));
        assert!(!is_valid_attribute_value("img", "alt", "a-->b"));
    }
}
