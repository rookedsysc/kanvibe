//! 참여자에 관한 문장: `participant`·`actor`(별칭, `@{ "type": ... }` 설정), `box ... end` 묶음,
//! `create`·`destroy`, 팝업 메뉴 문장(`links`·`link`·`properties`·`details`).

use super::css_color::is_css_color;
use super::document::{Participant, ParticipantGroup, ParticipantKind};
use super::parser::{SequenceParser, clean_text, split_keyword, syntax};
use crate::DiagramError;

/// 참여자 이름 뒤의 설명(별칭 글, 머리말이 정한 감싸기)
type Description = Option<(String, Option<bool>)>;

impl SequenceParser {
    /// addActor: 이미 있는 참여자는 별칭이 새로 올 때만 다시 쓴다
    pub(super) fn add_participant(
        &mut self,
        id: &str,
        description: Description,
        kind: ParticipantKind,
    ) -> Result<(), DiagramError> {
        let existing = self.participant_ids.get(id).copied();
        let mut group = self.open_group;
        if let Some(index) = existing {
            let old_group = self.document.participants[index].group;
            if group.is_some() && old_group.is_some() && group != old_group {
                return Err(syntax(&format!("{id} 참여자는 한 box에만 있어야 합니다")));
            }
            group = old_group.or(group);
            self.document.participants[index].group = group;
            if description.is_none() {
                return Ok(());
            }
        }
        let (description, wrap) = description.unwrap_or_else(|| (id.to_string(), None));
        let is_wrapped = wrap.unwrap_or(self.is_auto_wrapped);
        let participant = Participant {
            description,
            kind,
            previous: self.previous_participant,
            next: None,
            is_wrapped,
            group,
            link_labels: Vec::new(),
            has_custom_class: false,
        };
        let index = self.store_participant(id, existing, participant);
        if let Some(previous) = self.previous_participant {
            self.document.participants[previous].next = Some(index);
        }
        if let Some(open) = self.open_group {
            self.document.groups[open].members.push(index);
        }
        self.previous_participant = Some(index);
        Ok(())
    }

    fn store_participant(
        &mut self,
        id: &str,
        existing: Option<usize>,
        participant: Participant,
    ) -> usize {
        let participants = &mut self.document.participants;
        let index = match existing {
            Some(index) => {
                participants[index] = participant;
                index
            }
            None => {
                participants.push(participant);
                participants.len() - 1
            }
        };
        self.participant_ids.insert(id.to_string(), index);
        index
    }

    /// `이름 [@{ 설정 }] [as 별칭]`
    pub(super) fn parse_participant(
        &mut self,
        rest: &str,
        kind: ParticipantKind,
    ) -> Result<(), DiagramError> {
        let (id, description, kind) = read_declaration(rest.trim(), kind)?;
        if id.is_empty() {
            return Err(syntax("참여자 이름이 비었습니다"));
        }
        self.add_participant(&id, description, kind)
    }

    /// `create participant|actor 이름`: 다음 메시지가 이 참여자를 받아야 한다
    pub(super) fn parse_create(&mut self, rest: &str) -> Result<(), DiagramError> {
        let (keyword, declaration) = split_keyword(rest.trim());
        let kind = match keyword.as_str() {
            "participant" => ParticipantKind::Participant,
            "actor" => ParticipantKind::Actor,
            _ => return Err(syntax("create 뒤에는 participant나 actor가 와야 합니다")),
        };
        let (id, description, kind) = read_declaration(declaration.trim(), kind)?;
        if self.participant_ids.contains_key(&id) {
            return Err(syntax(&format!("이미 있는 참여자를 만듭니다: {id}")));
        }
        self.add_participant(&id, description, kind)?;
        let index = self.participant_ids[&id];
        self.last_created = Some(index);
        let event_index = self.document.events.len();
        self.document.created.insert(index, event_index);
        Ok(())
    }

    /// `destroy 이름`: 다음 메시지가 이 참여자에게서 나오거나 이 참여자로 가야 한다
    pub(super) fn parse_destroy(&mut self, rest: &str) -> Result<(), DiagramError> {
        let id = rest.trim();
        if id.is_empty() {
            return Err(syntax("destroy 뒤에 참여자 이름이 없습니다"));
        }
        self.last_destroyed = Some(id.to_string());
        let event_index = self.document.events.len();
        self.destroyed_names.push((id.to_string(), event_index));
        Ok(())
    }

    /// 생성·소멸 선언 바로 뒤 메시지는 그 참여자를 지나야 한다
    pub(super) fn check_lifecycle_message(
        &mut self,
        from: usize,
        to: usize,
    ) -> Result<(), DiagramError> {
        if let Some(created) = self.last_created.take() {
            if to != created {
                return Err(syntax("만든 참여자에게 가는 메시지가 바로 뒤에 없습니다"));
            }
            return Ok(());
        }
        let Some(name) = self.last_destroyed.take() else {
            return Ok(());
        };
        let destroyed = self.participant_ids.get(&name).copied();
        if destroyed != Some(to) && destroyed != Some(from) {
            return Err(syntax("없앤 참여자를 지나는 메시지가 바로 뒤에 없습니다"));
        }
        Ok(())
    }

    /// parseBoxData: 첫 낱말이 CSS 색이면 채움색, 나머지가 제목이다
    pub(super) fn open_participant_group(&mut self, rest: &str) -> Result<(), DiagramError> {
        let rest = rest.trim();
        let (color, title) = split_group_color(rest);
        let (fill, title) = if is_css_color(color) {
            (color.to_string(), title)
        } else {
            ("transparent".to_string(), rest)
        };
        let (title, wrap) = clean_text(title);
        let is_wrapped = wrap.unwrap_or(self.is_auto_wrapped);
        self.document.groups.push(ParticipantGroup {
            title: (!title.is_empty()).then_some(title),
            fill,
            is_wrapped,
            members: Vec::new(),
        });
        self.open_group = Some(self.document.groups.len() - 1);
        Ok(())
    }

    /// box 안에는 참여자 선언만 온다
    pub(super) fn parse_group_statement(
        &mut self,
        keyword: &str,
        rest: &str,
    ) -> Result<(), DiagramError> {
        match keyword {
            "participant" => self.parse_participant(rest, ParticipantKind::Participant),
            "actor" => self.parse_participant(rest, ParticipantKind::Actor),
            "destroy" => self.parse_destroy(rest),
            "end" => {
                self.open_group = None;
                Ok(())
            }
            _ => Err(syntax(&format!("box 안에 올 수 없는 문장: {keyword}"))),
        }
    }

    /// 팝업 메뉴 문장. 정적 그림에는 메뉴가 보이지 않으므로 메뉴 이름과 `class` 속성만 남긴다
    pub(super) fn parse_menu(&mut self, keyword: &str, rest: &str) -> Result<(), DiagramError> {
        let (name, text) = rest
            .split_once(':')
            .ok_or_else(|| syntax(&format!("{keyword} 문장에 `:` 글자가 없습니다")))?;
        let index = self.reference_participant(name.trim())?;
        let text = text.split('#').next().unwrap_or("").trim();
        let text = text.replace("&equals;", "=").replace("&amp;", "&");
        let participant = &mut self.document.participants[index];
        match keyword {
            "links" => {
                for label in json_object_keys(&text) {
                    add_link_label(participant, label);
                }
            }
            "link" => add_link_label(participant, link_label(&text)),
            "properties" => {
                participant.has_custom_class |=
                    json_object_keys(&text).iter().any(|key| key == "class");
            }
            _ => {}
        }
        Ok(())
    }
}

/// insertLinks: 같은 이름은 한 번만 남는다
fn add_link_label(participant: &mut Participant, label: String) {
    if !participant.link_labels.contains(&label) {
        participant.link_labels.push(label);
    }
}

/// addALink: `이름 @ 주소`에서 `@` 앞 한 글자를 뺀 앞부분
fn link_label(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let separator = characters.iter().position(|character| *character == '@');
    let end = match separator {
        Some(position) => position.saturating_sub(1),
        None => characters.len().saturating_sub(2),
    };
    characters[..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

/// JSON 객체의 열쇠들(읽지 못하면 비어 있다)
fn json_object_keys(text: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(object)) => object.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `rgb(...)`·`hsl(...)`나 첫 낱말(영숫자)을 색 후보로 떼어 낸다
fn split_group_color(rest: &str) -> (&str, &str) {
    let lower = rest.to_ascii_lowercase();
    let is_function = ["rgba", "rgb", "hsla", "hsl"]
        .iter()
        .any(|name| lower.starts_with(name) && lower[name.len()..].trim_start().starts_with('('));
    let end = if is_function {
        rest.rfind(')').map_or(rest.len(), |index| index + 1)
    } else {
        rest.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .unwrap_or(rest.len())
    };
    let color = rest[..end].trim();
    let color = if color.is_empty() {
        "transparent"
    } else {
        color
    };
    (color, &rest[end..])
}

/// 선언에서 이름·설명·모양을 읽는다
fn read_declaration(
    rest: &str,
    kind: ParticipantKind,
) -> Result<(String, Description, ParticipantKind), DiagramError> {
    let Some(config_start) = rest.find("@{") else {
        let (id, description) = split_alias(rest);
        return Ok((id.to_string(), description, kind));
    };
    let id = rest[..config_start].trim().to_string();
    let config_end = rest[config_start..]
        .find('}')
        .map(|offset| config_start + offset)
        .ok_or_else(|| syntax("참여자 설정이 `}`로 닫히지 않았습니다"))?;
    let (type_name, alias) = read_config(&rest[config_start + 2..config_end]);
    let kind = match type_name {
        Some(name) => ParticipantKind::from_type_name(&name).ok_or_else(|| {
            DiagramError::Unsupported(format!("sequenceDiagram 참여자 type: {name}"))
        })?,
        None => kind,
    };
    let after = rest[config_end + 1..].trim_start();
    let mut description = strip_as(after).map(clean_text);
    let is_alias_usable = description.as_ref().is_none_or(|(text, _)| *text == id);
    if let Some(alias) = alias.filter(|_| is_alias_usable) {
        let wrap = description.and_then(|(_, wrap)| wrap);
        description = Some((alias, wrap));
    }
    Ok((id, description, kind))
}

/// `이름 as 별칭`
fn split_alias(rest: &str) -> (&str, Description) {
    let id_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    match strip_as(rest[id_end..].trim_start()) {
        Some(alias) => (&rest[..id_end], Some(clean_text(alias))),
        None => (rest, None),
    }
}

fn strip_as(text: &str) -> Option<&str> {
    let keyword = text.get(..2)?;
    let after = &text[2..];
    let is_word = after.starts_with(char::is_whitespace);
    (keyword.eq_ignore_ascii_case("as") && is_word).then_some(after)
}

/// `@{ "type": "database", "alias": "DB" }`(YAML 흐름 표기)에서 type·alias만 읽는다
fn read_config(config: &str) -> (Option<String>, Option<String>) {
    let mut type_name = None;
    let mut alias = None;
    for entry in config.split(',') {
        let Some((key, value)) = entry.split_once(':') else {
            continue;
        };
        let unquote = |text: &str| text.trim().trim_matches(['"', '\'']).to_string();
        match unquote(key).as_str() {
            "type" => type_name = Some(unquote(value)),
            "alias" => alias = Some(unquote(value)),
            _ => {}
        }
    }
    (type_name, alias)
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::*;

    #[test]
    fn 설정과_별칭으로_모양을_정한다() {
        let document = parse(
            "sequenceDiagram\nparticipant D@{ \"type\": \"database\" } as Store\nparticipant Q@{ \"type\": \"queue\", \"alias\": \"Jobs\" }\nactor Casa as wrap:Long name",
        )
        .unwrap();
        let participants = &document.participants;
        assert_eq!(participants[0].kind, ParticipantKind::Database);
        assert_eq!(participants[0].description, "Store");
        assert_eq!(participants[1].kind, ParticipantKind::Queue);
        assert_eq!(participants[1].description, "Jobs");
        assert_eq!(participants[2].description, "Long name");
        assert!(participants[2].is_wrapped);
    }

    #[test]
    fn box는_색과_제목으로_참여자를_묶는다() {
        let document = parse(
            "sequenceDiagram\nbox Aqua Front end\nparticipant A\nend\nbox Back end\nparticipant B\nend\nA->>B: hi",
        )
        .unwrap();
        assert_eq!(document.groups[0].fill, "Aqua");
        assert_eq!(document.groups[0].title.as_deref(), Some("Front end"));
        assert_eq!(document.groups[1].fill, "transparent");
        assert_eq!(document.groups[1].title.as_deref(), Some("Back end"));
        assert_eq!(document.participants[1].group, Some(1));
    }

    #[test]
    fn 생성과_소멸은_바로_뒤_메시지를_요구한다() {
        let document = parse(
            "sequenceDiagram\nA->>B: x\ncreate participant C\nA->>C: new\ndestroy C\nC-->>A: bye",
        )
        .unwrap();
        assert_eq!(document.created.get(&2), Some(&1));
        assert_eq!(document.destroyed.get(&2), Some(&2));
        assert!(parse("sequenceDiagram\ncreate participant C\nA->>B: x").is_err());
    }

    #[test]
    fn 메뉴_문장은_이름만_남긴다() {
        let document = parse(
            "sequenceDiagram\nlinks A: {\"Dashboard\": \"https://x.io\", \"Wiki\": \"https://y.io\"}\nlink A: Repo @ https://z.io\nproperties A: {\"class\": \"internal\"}",
        )
        .unwrap();
        assert_eq!(
            document.participants[0].link_labels,
            ["Dashboard", "Wiki", "Repo"]
        );
        assert!(document.participants[0].has_custom_class);
    }
}
