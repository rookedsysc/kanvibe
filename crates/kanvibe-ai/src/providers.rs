//! 세션 기록을 읽는 AI provider 25종. 순서는 TS `AI_SESSION_PROVIDER_LABELS`와 같아 목록·집계 순서가 그대로 유지된다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiSessionProvider {
    Claude,
    Codebuddy,
    Qoder,
    Codex,
    Hermes,
    Pi,
    Omp,
    PrimeAgent,
    Cursor,
    Gemini,
    Antigravity,
    Rovo,
    Copilot,
    Opencode,
    Opencode2,
    Zcode,
    Grok,
    Openclaw,
    Devin,
    Droid,
    Cline,
    Kimi,
    Muse,
    Jcode,
    Kiro,
}

impl AiSessionProvider {
    pub const ALL: [AiSessionProvider; 25] = [
        Self::Claude,
        Self::Codebuddy,
        Self::Qoder,
        Self::Codex,
        Self::Hermes,
        Self::Pi,
        Self::Omp,
        Self::PrimeAgent,
        Self::Cursor,
        Self::Gemini,
        Self::Antigravity,
        Self::Rovo,
        Self::Copilot,
        Self::Opencode,
        Self::Opencode2,
        Self::Zcode,
        Self::Grok,
        Self::Openclaw,
        Self::Devin,
        Self::Droid,
        Self::Cline,
        Self::Kimi,
        Self::Muse,
        Self::Jcode,
        Self::Kiro,
    ];

    /// 저장·전송에 쓰는 식별자(`prime-agent` 등)
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codebuddy => "codebuddy",
            Self::Qoder => "qoder",
            Self::Codex => "codex",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::PrimeAgent => "prime-agent",
            Self::Cursor => "cursor",
            Self::Gemini => "gemini",
            Self::Antigravity => "antigravity",
            Self::Rovo => "rovo",
            Self::Copilot => "copilot",
            Self::Opencode => "opencode",
            Self::Opencode2 => "opencode2",
            Self::Zcode => "zcode",
            Self::Grok => "grok",
            Self::Openclaw => "openclaw",
            Self::Devin => "devin",
            Self::Droid => "droid",
            Self::Cline => "cline",
            Self::Kimi => "kimi",
            Self::Muse => "muse",
            Self::Jcode => "jcode",
            Self::Kiro => "kiro",
        }
    }

    /// 화면에 보이는 이름
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codebuddy => "CodeBuddy",
            Self::Qoder => "Qoder",
            Self::Codex => "Codex",
            Self::Hermes => "Hermes",
            Self::Pi => "Pi",
            Self::Omp => "OMP",
            Self::PrimeAgent => "Prime Agent",
            Self::Cursor => "Cursor",
            Self::Gemini => "Gemini",
            Self::Antigravity => "Antigravity",
            Self::Rovo => "Rovo Dev",
            Self::Copilot => "GitHub Copilot",
            Self::Opencode => "OpenCode",
            Self::Opencode2 => "OpenCode 2",
            Self::Zcode => "ZCode",
            Self::Grok => "Grok",
            Self::Openclaw => "OpenClaw",
            Self::Devin => "Devin",
            Self::Droid => "Droid",
            Self::Cline => "Cline",
            Self::Kimi => "Kimi",
            Self::Muse => "Muse",
            Self::Jcode => "Jcode",
            Self::Kiro => "Kiro",
        }
    }

    /// TS `isAiSessionProvider`. 알려진 식별자가 아니면 `None`이다
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.id() == value)
    }
}

impl std::fmt::Display for AiSessionProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_round_trip_through_serde_and_parse() {
        for provider in AiSessionProvider::ALL {
            let serialized = serde_json::to_string(&provider).unwrap();
            assert_eq!(serialized, format!("\"{}\"", provider.id()));
            assert_eq!(AiSessionProvider::parse(provider.id()), Some(provider));
        }
        assert_eq!(AiSessionProvider::parse("toString"), None);
    }
}
