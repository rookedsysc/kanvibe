//! SequenceDB가 모으는 값: 참여자·참여자 묶음(box)·사건(메시지 배열)·제목·생성/소멸 자리.

use std::collections::HashMap;

/// 참여자 모양(PARTICIPANT_TYPE)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ParticipantKind {
    Participant,
    Actor,
    Boundary,
    Control,
    Entity,
    Database,
    Collections,
    Queue,
}

impl ParticipantKind {
    /// `@{ "type": ... }`의 값
    pub fn from_type_name(name: &str) -> Option<Self> {
        let kind = match name {
            "participant" => Self::Participant,
            "actor" => Self::Actor,
            "boundary" => Self::Boundary,
            "control" => Self::Control,
            "entity" => Self::Entity,
            "database" => Self::Database,
            "collections" => Self::Collections,
            "queue" => Self::Queue,
            _ => return None,
        };
        Some(kind)
    }

    /// 생성·소멸 메시지가 상자 반폭 대신 그림 반폭(18px)만큼 비켜 가는 모양
    pub fn is_figure(self) -> bool {
        matches!(
            self,
            Self::Actor | Self::Control | Self::Entity | Self::Database
        )
    }
}

#[derive(Clone, Debug)]
pub(super) struct Participant {
    pub description: String,
    pub kind: ParticipantKind,
    pub previous: Option<usize>,
    pub next: Option<usize>,
    /// 설명 앞에 `wrap:`을 붙여 폭 안에서 줄을 바꾸는 참여자
    pub is_wrapped: bool,
    /// 속한 참여자 묶음 순번
    pub group: Option<usize>,
    /// `link`·`links`로 붙인 팝업 메뉴 항목 이름(그리지는 않지만 viewBox 크기에 든다)
    pub link_labels: Vec<String>,
    /// `properties`의 `class`로 기본 `.actor` 모양을 잃은 참여자
    pub has_custom_class: bool,
}

/// `box 색 제목 ... end`로 묶은 참여자들
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ParticipantGroup {
    pub title: Option<String>,
    /// CSS 색 글(알아볼 수 없으면 `transparent`)
    pub fill: String,
    pub is_wrapped: bool,
    pub members: Vec<usize>,
}

/// 메시지 화살촉. 반쪽 화살촉은 선 위(Top)나 아래(Bottom) 반만 그린다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArrowHead {
    Open,
    Filled,
    Cross,
    Async,
    Bidirectional,
    SolidTop,
    SolidBottom,
    StickTop,
    StickBottom,
    /// 화살촉이 보내는 쪽(선 시작)에 붙는 반쪽 화살촉
    ReverseSolidTop,
    ReverseSolidBottom,
    ReverseStickTop,
    ReverseStickBottom,
}

/// 메시지 선 종류(LINETYPE의 화살표 부분)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Arrow {
    pub head: ArrowHead,
    pub is_dotted: bool,
}

impl Arrow {
    pub fn is_bidirectional(self) -> bool {
        self.head == ArrowHead::Bidirectional
    }

    pub fn is_reverse(self) -> bool {
        matches!(
            self.head,
            ArrowHead::ReverseSolidTop
                | ArrowHead::ReverseSolidBottom
                | ArrowHead::ReverseStickTop
                | ArrowHead::ReverseStickBottom
        )
    }

    /// 받는 쪽 끝을 화살촉 길이(3px)만큼 당기지 않는 선
    pub fn keeps_stop_end(self) -> bool {
        self.is_reverse()
            || matches!(
                self.head,
                ArrowHead::Open | ArrowHead::StickTop | ArrowHead::StickBottom
            )
    }

    /// 보내는 쪽 끝도 화살촉 길이만큼 당기는 선
    pub fn pulls_start_end(self) -> bool {
        matches!(
            self.head,
            ArrowHead::Bidirectional | ArrowHead::ReverseSolidTop | ArrowHead::ReverseSolidBottom
        )
    }
}

/// 중앙 연결 동그라미 `()`가 붙는 쪽
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CentralConnection {
    /// `A->>()B`: 받는 쪽
    Target,
    /// `A()->>B`: 보내는 쪽
    Source,
    /// `A()->>()B`: 양쪽
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NotePlacement {
    LeftOf,
    RightOf,
    Over,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BlockKind {
    Loop,
    Alt,
    Opt,
    Par,
    Critical,
    Break,
    Rect,
}

impl BlockKind {
    /// 블록 왼쪽 위 이름표에 쓰는 낱말
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Loop => "loop",
            Self::Alt => "alt",
            Self::Opt => "opt",
            Self::Par => "par",
            Self::Critical => "critical",
            Self::Break => "break",
            Self::Rect => "rect",
        }
    }

    /// 이 블록 안에서 구역을 나누는 낱말(alt의 else, par의 and, critical의 option)
    pub fn section_keyword(self) -> Option<&'static str> {
        match self {
            Self::Alt => Some("else"),
            Self::Par => Some("and"),
            Self::Critical => Some("option"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SequenceMessage {
    pub from: usize,
    pub to: usize,
    pub text: String,
    pub arrow: Arrow,
    /// `->>+`·`->>()`처럼 받는 쪽을 활성화하는 메시지
    pub is_activating: bool,
    pub is_wrapped: bool,
    pub central: Option<CentralConnection>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SequenceNote {
    pub from: usize,
    pub to: usize,
    pub placement: NotePlacement,
    pub text: String,
    pub is_wrapped: bool,
}

/// SequenceDB의 messages 배열 한 칸. 배열 순번이 Mermaid의 메시지 id다
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SequenceEvent {
    Message(SequenceMessage),
    Note(SequenceNote),
    ActivationStart(usize),
    ActivationEnd(usize),
    /// 중앙 연결이 여는 활성화. 끝 사건이 없으면 그려지지 않고 메시지 경계만 바꾼다
    CentralActivation(usize),
    BlockStart {
        kind: BlockKind,
        text: String,
    },
    BlockSection(String),
    BlockEnd(BlockKind),
    Autonumber {
        start: Option<f64>,
        step: Option<f64>,
        is_visible: bool,
    },
}

#[derive(Debug, Default)]
pub(super) struct SequenceDocument {
    pub title: Option<String>,
    pub participants: Vec<Participant>,
    pub groups: Vec<ParticipantGroup>,
    pub events: Vec<SequenceEvent>,
    /// `create`한 참여자 → 그 참여자를 만드는 메시지의 사건 순번
    pub created: HashMap<usize, usize>,
    /// `destroy`한 참여자 → 그 참여자를 없애는 메시지의 사건 순번
    pub destroyed: HashMap<usize, usize>,
}
