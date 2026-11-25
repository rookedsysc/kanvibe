//! KaTeX(`output: "mathml"`, `strict: "error"`, `trust: false`) 결과에 해당하는 수식 트리.
//!
//! 앱이 실제로 쓰는 부분집합만 옮겼다: 글자·숫자·연산자, `^`·`_`, `{}` 묶음, `\frac`류, `\sqrt[n]{}`,
//! 그리스 문자, 자주 쓰는 기호·화살표·관계, 큰 연산자(`\sum` 등), 함수 이름(`\sin` 등), `\text`·`\mathrm`,
//! 글꼴 명령(`\mathbf` 등), 강세(`\hat` 등), 간격(`\,` 등), `\left…\right`.
//! 그 밖의 명령과 환경(`\begin{…}`), 수식 모드의 한글처럼 KaTeX strict 모드가 거절하는 입력은 오류로 돌려
//! TS처럼 원문을 `<code>`로 보여 주게 한다. `maxExpand`·`maxSize`가 막는 매크로·크기 명령은 없다.

#[derive(Clone, Debug, PartialEq)]
pub enum MathNode {
    Row(Vec<MathNode>),
    /// `<mi>`. 한 글자 변수는 기울이고(`is_italic`), 함수 이름·글꼴 명령은 바로 세운다
    Identifier {
        text: String,
        is_italic: bool,
    },
    Number(String),
    /// `<mo>`. 큰 연산자(∑ 등)는 `is_large`, `\left…\right` 괄호는 `is_stretchy`
    Operator {
        text: String,
        is_large: bool,
        is_stretchy: bool,
    },
    Text(String),
    Fraction {
        numerator: Box<MathNode>,
        denominator: Box<MathNode>,
    },
    Scripts {
        base: Box<MathNode>,
        subscript: Option<Box<MathNode>>,
        superscript: Option<Box<MathNode>>,
    },
    Sqrt {
        index: Option<Box<MathNode>>,
        radicand: Box<MathNode>,
    },
    Accent {
        base: Box<MathNode>,
        accent: char,
    },
    /// em 단위 간격
    Space(f32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathError(pub String);

pub fn parse_tex(source: &str) -> Result<MathNode, MathError> {
    let mut parser = TexParser {
        characters: source.chars().collect(),
        position: 0,
    };
    let nodes = parser.expression(None)?;
    if parser.position < parser.characters.len() {
        return Err(MathError(format!(
            "예상하지 못한 글자: {}",
            parser.characters[parser.position]
        )));
    }
    Ok(MathNode::Row(nodes))
}

struct TexParser {
    characters: Vec<char>,
    position: usize,
}

const GREEK: [(&str, char); 40] = [
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ϵ'),
    ("varepsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("vartheta", 'ϑ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("pi", 'π'),
    ("varpi", 'ϖ'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'ϕ'),
    ("varphi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Xi", 'Ξ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Upsilon", 'Υ'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
    ("ell", 'ℓ'),
    ("hbar", 'ℏ'),
];

const SYMBOL_OPERATORS: [(&str, &str); 52] = [
    ("cdot", "⋅"),
    ("times", "×"),
    ("div", "÷"),
    ("pm", "±"),
    ("mp", "∓"),
    ("le", "≤"),
    ("leq", "≤"),
    ("ge", "≥"),
    ("geq", "≥"),
    ("ne", "≠"),
    ("neq", "≠"),
    ("approx", "≈"),
    ("equiv", "≡"),
    ("sim", "∼"),
    ("simeq", "≃"),
    ("propto", "∝"),
    ("to", "→"),
    ("rightarrow", "→"),
    ("leftarrow", "←"),
    ("gets", "←"),
    ("Rightarrow", "⇒"),
    ("Leftarrow", "⇐"),
    ("leftrightarrow", "↔"),
    ("Leftrightarrow", "⇔"),
    ("implies", "⟹"),
    ("iff", "⟺"),
    ("mapsto", "↦"),
    ("in", "∈"),
    ("notin", "∉"),
    ("ni", "∋"),
    ("subset", "⊂"),
    ("subseteq", "⊆"),
    ("supset", "⊃"),
    ("supseteq", "⊇"),
    ("cup", "∪"),
    ("cap", "∩"),
    ("setminus", "∖"),
    ("land", "∧"),
    ("wedge", "∧"),
    ("lor", "∨"),
    ("vee", "∨"),
    ("neg", "¬"),
    ("lnot", "¬"),
    ("circ", "∘"),
    ("star", "⋆"),
    ("ast", "∗"),
    ("ldots", "…"),
    ("dots", "…"),
    ("cdots", "⋯"),
    ("vdots", "⋮"),
    ("ll", "≪"),
    ("gg", "≫"),
];

const IDENTIFIER_SYMBOLS: [(&str, &str); 6] = [
    ("infty", "∞"),
    ("partial", "∂"),
    ("nabla", "∇"),
    ("forall", "∀"),
    ("exists", "∃"),
    ("emptyset", "∅"),
];

const DELIMITERS: [(&str, &str); 12] = [
    ("langle", "⟨"),
    ("rangle", "⟩"),
    ("lfloor", "⌊"),
    ("rfloor", "⌋"),
    ("lceil", "⌈"),
    ("rceil", "⌉"),
    ("lvert", "|"),
    ("rvert", "|"),
    ("lVert", "‖"),
    ("rVert", "‖"),
    ("vert", "|"),
    ("Vert", "‖"),
];

const LARGE_OPERATORS: [(&str, &str); 8] = [
    ("sum", "∑"),
    ("prod", "∏"),
    ("int", "∫"),
    ("iint", "∬"),
    ("oint", "∮"),
    ("bigcup", "⋃"),
    ("bigcap", "⋂"),
    ("coprod", "∐"),
];

const FUNCTION_NAMES: [&str; 22] = [
    "sin", "cos", "tan", "cot", "sec", "csc", "log", "ln", "exp", "lim", "min", "max", "det",
    "gcd", "arg", "deg", "dim", "ker", "sup", "inf", "sinh", "cosh",
];

const ACCENTS: [(&str, char); 7] = [
    ("hat", '^'),
    ("bar", '¯'),
    ("overline", '¯'),
    ("vec", '→'),
    ("tilde", '~'),
    ("dot", '˙'),
    ("ddot", '¨'),
];

fn lookup<'a, T: Copy>(table: &'a [(&'a str, T)], name: &str) -> Option<T> {
    table
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

fn spacing(name: &str) -> Option<f32> {
    match name {
        "," => Some(3.0 / 18.0),
        ":" | ">" => Some(4.0 / 18.0),
        ";" => Some(5.0 / 18.0),
        "!" => Some(-3.0 / 18.0),
        " " => Some(0.25),
        "quad" => Some(1.0),
        "qquad" => Some(2.0),
        _ => None,
    }
}

impl TexParser {
    fn peek(&self) -> Option<char> {
        self.characters.get(self.position).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.position += 1;
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), MathError> {
        self.skip_whitespace();
        if self.peek() == Some(expected) {
            self.position += 1;
            return Ok(());
        }
        Err(MathError(format!("`{expected}`가 필요합니다")))
    }

    fn expression(&mut self, stop: Option<char>) -> Result<Vec<MathNode>, MathError> {
        let mut nodes: Vec<MathNode> = Vec::new();
        loop {
            self.skip_whitespace();
            let Some(character) = self.peek() else {
                return if stop.is_some() {
                    Err(MathError("묶음이 닫히지 않았습니다".to_string()))
                } else {
                    Ok(nodes)
                };
            };
            if Some(character) == stop {
                self.position += 1;
                return Ok(nodes);
            }
            if self.at_right_delimiter() {
                return Ok(nodes);
            }
            if character == '^' || character == '_' {
                self.position += 1;
                let base = nodes.pop().unwrap_or(MathNode::Row(Vec::new()));
                let script = self.argument()?;
                nodes.push(attach_script(base, character == '^', script)?);
                continue;
            }
            nodes.push(self.atom()?);
        }
    }

    fn at_right_delimiter(&self) -> bool {
        let rest: String = self.characters[self.position..].iter().take(6).collect();
        rest == "\\right"
    }

    /// `^`·`_`·명령 인자: 묶음 하나 또는 한 토큰
    fn argument(&mut self) -> Result<MathNode, MathError> {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => {
                self.position += 1;
                Ok(MathNode::Row(self.expression(Some('}'))?))
            }
            Some(_) => self.atom(),
            None => Err(MathError("인자가 필요합니다".to_string())),
        }
    }

    fn raw_group(&mut self) -> Result<String, MathError> {
        self.expect('{')?;
        let mut depth = 1;
        let mut text = String::new();
        while let Some(character) = self.peek() {
            self.position += 1;
            depth += match character {
                '{' => 1,
                '}' => -1,
                _ => 0,
            };
            if depth == 0 {
                return Ok(text);
            }
            text.push(character);
        }
        Err(MathError("묶음이 닫히지 않았습니다".to_string()))
    }

    fn atom(&mut self) -> Result<MathNode, MathError> {
        let character = self
            .peek()
            .ok_or_else(|| MathError("글자가 필요합니다".to_string()))?;
        self.position += 1;
        match character {
            '\\' => self.command(),
            '{' => Ok(MathNode::Row(self.expression(Some('}'))?)),
            '~' => Ok(MathNode::Space(0.25)),
            '0'..='9' | '.' => Ok(MathNode::Number(self.number(character))),
            'a'..='z' | 'A'..='Z' => Ok(MathNode::Identifier {
                text: character.to_string(),
                is_italic: true,
            }),
            '\'' => Ok(operator("′")),
            '+' | '-' | '=' | '<' | '>' | ',' | ';' | ':' | '!' | '(' | ')' | '[' | ']' | '|'
            | '/' | '*' | '?' | '@' => Ok(operator(&character.replace_minus())),
            '}' | '&' | '#' | '%' | '$' => {
                Err(MathError(format!("여기에 올 수 없는 글자: {character}")))
            }
            other if other.is_alphabetic() => {
                Err(MathError(format!("수식 모드의 유니코드 글자: {other}")))
            }
            other => Ok(operator(&other.to_string())),
        }
    }

    fn number(&mut self, first: char) -> String {
        let mut text = first.to_string();
        while let Some(next) = self
            .peek()
            .filter(|next| next.is_ascii_digit() || *next == '.')
        {
            text.push(next);
            self.position += 1;
        }
        text
    }

    fn command_name(&mut self) -> String {
        let mut name = String::new();
        while let Some(next) = self.peek().filter(char::is_ascii_alphabetic) {
            name.push(next);
            self.position += 1;
        }
        if name.is_empty()
            && let Some(next) = self.peek()
        {
            name.push(next);
            self.position += 1;
        }
        name
    }

    fn command(&mut self) -> Result<MathNode, MathError> {
        let name = self.command_name();
        if let Some(node) = self.structural_command(&name)? {
            return Ok(node);
        }
        if let Some(letter) = lookup(&GREEK, &name) {
            let is_italic = letter.is_lowercase();
            return Ok(MathNode::Identifier {
                text: letter.to_string(),
                is_italic,
            });
        }
        if let Some(symbol) =
            lookup(&SYMBOL_OPERATORS, &name).or_else(|| lookup(&DELIMITERS, &name))
        {
            return Ok(operator(symbol));
        }
        if let Some(symbol) = lookup(&IDENTIFIER_SYMBOLS, &name) {
            return Ok(MathNode::Identifier {
                text: symbol.to_string(),
                is_italic: false,
            });
        }
        if let Some(symbol) = lookup(&LARGE_OPERATORS, &name) {
            return Ok(MathNode::Operator {
                text: symbol.to_string(),
                is_large: true,
                is_stretchy: false,
            });
        }
        if FUNCTION_NAMES.contains(&name.as_str()) {
            return Ok(MathNode::Identifier {
                text: name,
                is_italic: false,
            });
        }
        if let Some(width) = spacing(&name) {
            return Ok(MathNode::Space(width));
        }
        match name.as_str() {
            "{" | "}" | "|" | "_" | "#" | "%" | "&" | "$" => {
                Ok(operator(if name == "|" { "‖" } else { &name }))
            }
            _ => Err(MathError(format!("지원하지 않는 명령: \\{name}"))),
        }
    }

    fn structural_command(&mut self, name: &str) -> Result<Option<MathNode>, MathError> {
        let node = match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let numerator = Box::new(self.argument()?);
                let denominator = Box::new(self.argument()?);
                MathNode::Fraction {
                    numerator,
                    denominator,
                }
            }
            "sqrt" => self.square_root()?,
            "text" | "textrm" | "textnormal" | "mbox" => MathNode::Text(self.raw_group()?),
            "mathrm" | "operatorname" | "mathbf" | "boldsymbol" | "mathit" | "mathbb"
            | "mathcal" | "mathsf" | "mathtt" => {
                let text = self.raw_group()?;
                if !text.is_ascii() {
                    return Err(MathError("글꼴 명령 안의 유니코드 글자".to_string()));
                }
                MathNode::Identifier {
                    text,
                    is_italic: name == "mathit",
                }
            }
            "left" => self.fenced()?,
            _ => match lookup(&ACCENTS, name) {
                Some(accent) => MathNode::Accent {
                    base: Box::new(self.argument()?),
                    accent,
                },
                None => return Ok(None),
            },
        };
        Ok(Some(node))
    }

    fn square_root(&mut self) -> Result<MathNode, MathError> {
        self.skip_whitespace();
        let index = if self.peek() == Some('[') {
            self.position += 1;
            Some(Box::new(MathNode::Row(self.expression(Some(']'))?)))
        } else {
            None
        };
        Ok(MathNode::Sqrt {
            index,
            radicand: Box::new(self.argument()?),
        })
    }

    fn delimiter(&mut self) -> Result<String, MathError> {
        self.skip_whitespace();
        let character = self
            .peek()
            .ok_or_else(|| MathError("괄호가 필요합니다".to_string()))?;
        self.position += 1;
        if character != '\\' {
            return Ok(if character == '.' {
                String::new()
            } else {
                character.to_string()
            });
        }
        let name = self.command_name();
        match name.as_str() {
            "{" | "}" => Ok(name),
            "|" => Ok("‖".to_string()),
            _ => lookup(&DELIMITERS, &name)
                .map(str::to_string)
                .ok_or_else(|| MathError(format!("괄호가 아닙니다: \\{name}"))),
        }
    }

    fn fenced(&mut self) -> Result<MathNode, MathError> {
        let open = self.delimiter()?;
        let mut content = self.expression(None)?;
        if !self.at_right_delimiter() {
            return Err(MathError("\\right가 필요합니다".to_string()));
        }
        self.position += "\\right".len();
        let close = self.delimiter()?;
        let mut row = Vec::new();
        if !open.is_empty() {
            row.push(MathNode::Operator {
                text: open,
                is_large: false,
                is_stretchy: true,
            });
        }
        row.append(&mut content);
        if !close.is_empty() {
            row.push(MathNode::Operator {
                text: close,
                is_large: false,
                is_stretchy: true,
            });
        }
        Ok(MathNode::Row(row))
    }
}

trait MinusSign {
    fn replace_minus(self) -> String;
}

impl MinusSign for char {
    /// KaTeX는 `-`를 수학 빼기 기호(U+2212)로 바꾼다
    fn replace_minus(self) -> String {
        if self == '-' {
            "−".to_string()
        } else {
            self.to_string()
        }
    }
}

fn operator(text: &str) -> MathNode {
    MathNode::Operator {
        text: text.to_string(),
        is_large: false,
        is_stretchy: false,
    }
}

fn attach_script(
    base: MathNode,
    is_superscript: bool,
    script: MathNode,
) -> Result<MathNode, MathError> {
    let script = Some(Box::new(script));
    match base {
        MathNode::Scripts {
            base,
            subscript,
            superscript,
        } => {
            let has_conflict = if is_superscript {
                superscript.is_some()
            } else {
                subscript.is_some()
            };
            if has_conflict {
                return Err(MathError("첨자가 두 번 붙었습니다".to_string()));
            }
            Ok(if is_superscript {
                MathNode::Scripts {
                    base,
                    subscript,
                    superscript: script,
                }
            } else {
                MathNode::Scripts {
                    base,
                    subscript: script,
                    superscript,
                }
            })
        }
        other => Ok(if is_superscript {
            MathNode::Scripts {
                base: Box::new(other),
                subscript: None,
                superscript: script,
            }
        } else {
            MathNode::Scripts {
                base: Box::new(other),
                subscript: script,
                superscript: None,
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 위첨자와_분수를_읽는다() {
        let node = parse_tex("E=mc^2").expect("parse");
        let MathNode::Row(items) = node else {
            panic!("row")
        };
        assert_eq!(items.len(), 4);
        assert!(matches!(
            items[3],
            MathNode::Scripts {
                superscript: Some(_),
                ..
            }
        ));
        assert!(
            matches!(parse_tex("\\frac{a}{b}").expect("frac"), MathNode::Row(ref inner) if matches!(inner[0], MathNode::Fraction { .. }))
        );
    }

    #[test]
    fn 모르는_명령과_한글은_거절한다() {
        assert!(parse_tex("\\begin{matrix}a\\end{matrix}").is_err());
        assert!(parse_tex("가+1").is_err());
        assert!(parse_tex("x^2^3").is_err());
    }
}
