//! Monaco TypeScript 언어 설정(`basic-languages/typescript/typescript.ts`의 `conf`) 가운데 괄호·따옴표 밖의 규칙:
//! 문서 주석 `/**` 자동 닫기, 문서 주석 안 Enter(`onEnterRules`), `// #region`·`// #endregion` 접기 표식(`folding.markers`).
//! JavaScript 언어는 같은 `conf`를 쓴다. 다른 언어에는 이 규칙이 없다(Monaco와 같다).
//! 정규식은 Monaco 원문 그대로이고, JS `RegExp`와 같은 의미로 도는 `regress`로 맞춘다.

use regress::Regex;

use super::syntax::Language;

/// `autoClosingPairs`의 `{ open: "/**", close: " */", notIn: ["string"] }`
pub const DOC_COMMENT_OPEN: &str = "/**";
pub const DOC_COMMENT_CLOSE: &str = " */";

/// 문서 주석 시작 줄(`/** ...|`). 앞 두 규칙이 함께 쓴다
const DOC_COMMENT_START: &str = r"^\s*\/\*\*(?!\/)([^\*]|\*(?!\/))*$";
/// `/** | */`의 커서 뒤
const DOC_COMMENT_END_AFTER_CURSOR: &str = r"^\s*\*\/$";
/// 문서 주석 가운데 줄(` * ...|`)
const DOC_COMMENT_MIDDLE: &str = r"^(\t|(\ \ ))*\ \*(\ ([^\*]|\*(?!\/))*)?$";
/// 문서 주석 끝 줄(` */|`)
const DOC_COMMENT_END: &str = r"^(\t|(\ \ ))*\ \*\/\s*$";
/// `folding.markers`의 start·end를 Monaco `computeRanges`처럼 하나로 묶은 것. 첫 묶음이 맞으면 시작 표식이다
const REGION_MARKERS: &str = r"(^\s*//\s*#?region\b)|(?:^\s*//\s*#?endregion\b)";

pub fn has_typescript_config(language: Language) -> bool {
    matches!(language, Language::TypeScript | Language::JavaScript)
}

fn is_match(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).is_ok_and(|regex| regex.find(text).is_some())
}

/// Enter를 눌렀을 때 할 일(Monaco `EnterAction`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnterAction {
    /// 새 줄 뒤에 한 줄을 더 넣어 커서 뒤 글자를 원래 들여쓰기로 내린다(`IndentAction.IndentOutdent`)
    pub is_indent_outdent: bool,
    /// 새 줄 들여쓰기 뒤에 붙일 글자
    pub append_text: &'static str,
    /// 들여쓰기 끝에서 지울 글자 수
    pub remove_text: usize,
}

/// `onEnterRules`를 순서대로 맞춰 본다. `before`는 줄 처음부터 커서까지, `after`는 커서부터 줄 끝까지다
pub fn enter_action(language: Language, before: &str, after: &str) -> Option<EnterAction> {
    if !has_typescript_config(language) {
        return None;
    }
    let action = |is_indent_outdent, append_text, remove_text| EnterAction {
        is_indent_outdent,
        append_text,
        remove_text,
    };
    if is_match(DOC_COMMENT_START, before) {
        let is_before_end = is_match(DOC_COMMENT_END_AFTER_CURSOR, after);
        return Some(action(is_before_end, " * ", 0));
    }
    if is_match(DOC_COMMENT_MIDDLE, before) {
        return Some(action(false, "* ", 0));
    }
    is_match(DOC_COMMENT_END, before).then(|| action(false, "", 1))
}

/// 줄이 접기 표식인지. `Some(true)`는 `// #region`, `Some(false)`는 `// #endregion`
pub fn region_marker(language: Language, line: &str) -> Option<bool> {
    if !has_typescript_config(language) {
        return None;
    }
    let regex = Regex::new(REGION_MARKERS).ok()?;
    let found = regex.find(line)?;
    Some(found.group(1).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_comment_enter_rules_follow_the_monaco_typescript_config() {
        let outdent = enter_action(Language::TypeScript, "  /**", " */").unwrap();
        assert!(outdent.is_indent_outdent);
        assert_eq!(outdent.append_text, " * ");

        let open = enter_action(Language::TypeScript, "/** Sum", "").unwrap();
        assert!(!open.is_indent_outdent);
        assert_eq!(open.append_text, " * ");

        let middle = enter_action(Language::JavaScript, "   * line", "").unwrap();
        assert_eq!(middle.append_text, "* ");

        let end = enter_action(Language::TypeScript, "   */", "").unwrap();
        assert_eq!((end.append_text, end.remove_text), ("", 1));

        assert_eq!(enter_action(Language::TypeScript, "/** done */", ""), None);
        assert_eq!(enter_action(Language::TypeScript, "/*", ""), None);
        assert_eq!(enter_action(Language::Rust, "/**", ""), None);
    }

    #[test]
    fn region_markers_are_recognized_only_for_typescript_family() {
        assert_eq!(
            region_marker(Language::TypeScript, "  // #region helpers"),
            Some(true)
        );
        assert_eq!(region_marker(Language::TypeScript, "//region"), Some(true));
        assert_eq!(
            region_marker(Language::JavaScript, "// #endregion"),
            Some(false)
        );
        assert_eq!(region_marker(Language::TypeScript, "// #regional"), None);
        assert_eq!(region_marker(Language::TypeScript, "x // #region"), None);
        assert_eq!(region_marker(Language::Python, "// #region"), None);
    }
}
