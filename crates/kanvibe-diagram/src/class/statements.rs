//! classDB의 묶음·스타일 문장: `namespace`(addNamespace), `style`(setCssStyle), `classDef`(defineClass),
//! `cssClass`·`:::`(setCssClass).

use super::parser::{ClassDiagram, Namespace};

impl ClassDiagram {
    /// addNamespace: 열린 namespace 안이면 `바깥.이름`으로 잇고, 점으로 나뉜 조상 묶음을 먼저 만든다
    pub(super) fn add_namespace(&mut self, name: &str, open: &[String]) -> String {
        let qualified = match open.last() {
            Some(outer) => format!("{outer}.{name}"),
            None => name.to_string(),
        };
        let mut ancestor: Option<String> = None;
        for part in qualified.split('.') {
            let id = match &ancestor {
                Some(outer) => format!("{outer}.{part}"),
                None => part.to_string(),
            };
            if !self.namespaces.iter().any(|namespace| namespace.id == id) {
                self.namespaces.push(Namespace {
                    id: id.clone(),
                    label: part.to_string(),
                    parent: ancestor.clone(),
                });
            }
            ancestor = Some(id);
        }
        qualified
    }

    /// `style`·`classDef`·`cssClass` 문장이면 읽고 참을 돌려준다
    pub(super) fn parse_style_statement(&mut self, line: &str) -> bool {
        if let Some(rest) = line.strip_prefix("style ") {
            let (id, declarations) = split_first_word(rest);
            self.set_css_style(id, declarations);
        } else if let Some(rest) = line.strip_prefix("classDef ") {
            let (ids, declarations) = split_first_word(rest);
            for id in ids.split(',') {
                self.define_class(id.trim(), declarations);
            }
        } else if let Some(rest) = line.strip_prefix("cssClass ") {
            let (ids, class_name) = split_first_word(rest);
            for id in ids.trim_matches('"').split(',') {
                self.set_css_class(id.trim(), class_name.trim());
            }
        } else {
            return false;
        }
        true
    }

    /// setCssClass: 클래스의 cssClasses에 이름을 덧붙인다
    pub(super) fn set_css_class(&mut self, id: &str, class_name: &str) {
        if let Some(class) = self.class_mut(id) {
            class.css_classes.push(' ');
            class.css_classes.push_str(class_name);
        }
    }

    /// setCssStyle: 쉼표로 나눈 선언을 그 클래스에 넣는다
    fn set_css_style(&mut self, id: &str, declarations: &str) {
        if let Some(class) = self.class_mut(id) {
            class.styles.extend(split_declarations(declarations));
        }
    }

    /// defineClass: 지금까지 cssClasses 문자열에 그 이름이 들어 있는 클래스마다 선언을 덧붙인다
    fn define_class(&mut self, class_name: &str, declarations: &str) {
        let styles = split_declarations(declarations);
        for class in &mut self.classes {
            if class.css_classes.contains(class_name) {
                class.styles.extend(styles.iter().cloned());
            }
        }
    }
}

/// `이름 나머지` → (`이름`, `나머지`)
fn split_first_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    match text.split_once(char::is_whitespace) {
        Some((first, rest)) => (first, rest.trim()),
        None => (text, ""),
    }
}

fn split_declarations(declarations: &str) -> Vec<String> {
    declarations
        .split(',')
        .map(|declaration| declaration.trim().trim_end_matches(';').to_string())
        .filter(|declaration| !declaration.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::class::parser::parse;

    #[test]
    fn 스타일과_클래스_정의를_붙인다() {
        let diagram = parse(
            "classDiagram\n class A:::warm\n class B\n cssClass \"B\" warm\n classDef warm fill:#f96,color:#fff\n style A stroke:#333,stroke-width:4px",
        )
        .expect("parse");
        assert_eq!(diagram.classes[0].css_classes, "default warm");
        assert_eq!(
            diagram.classes[0].styles,
            vec!["fill:#f96", "color:#fff", "stroke:#333", "stroke-width:4px"]
        );
        assert_eq!(diagram.classes[1].styles, vec!["fill:#f96", "color:#fff"]);
    }

    #[test]
    fn 점_이름_namespace는_조상_묶음을_만든다() {
        let diagram = parse(
            "classDiagram\n namespace Shapes.Round {\n class Circle\n }\n namespace Other {\n class Square\n }\n Circle --> Square",
        )
        .expect("parse");
        let ids: Vec<&str> = diagram
            .namespaces
            .iter()
            .map(|namespace| namespace.id.as_str())
            .collect();
        assert_eq!(ids, vec!["Shapes", "Shapes.Round", "Other"]);
        assert_eq!(diagram.namespaces[1].label, "Round");
        assert_eq!(diagram.classes[0].parent.as_deref(), Some("Shapes.Round"));
        assert_eq!(diagram.relations.len(), 1);
    }
}
