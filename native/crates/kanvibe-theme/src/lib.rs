use kanvibe_core::{TaskPriority, TaskStatus};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const fn from_hex(hex: u32) -> Self {
        Self {
            red: ((hex >> 16) & 0xff) as u8,
            green: ((hex >> 8) & 0xff) as u8,
            blue: (hex & 0xff) as u8,
        }
    }
}

pub const PRIMARY: Rgb = Rgb::from_hex(0x0064ff);
pub const NEUTRAL_BUTTON_SURFACE: Rgb = Rgb::from_hex(0x202632);
pub const TAG_BRANCH_TEXT: Rgb = Rgb::from_hex(0xaeb3bd);
pub const TAG_SESSION_TEXT: Rgb = Rgb::from_hex(0x202124);
pub const TAG_SSH_TEXT: Rgb = Rgb::from_hex(0x9be6b4);
pub const PROJECT_COLOR_FALLBACK: Rgb = Rgb::from_hex(0x94a3b8);

pub const fn priority_tag_color(priority: TaskPriority) -> Rgb {
    match priority {
        TaskPriority::Low => PRIMARY,
        TaskPriority::Medium => Rgb::from_hex(0xffd58a),
        TaskPriority::High => Rgb::from_hex(0xffaaa2),
    }
}

pub fn agent_tag_color(agent: &str) -> Rgb {
    match agent {
        "claude" => Rgb::from_hex(0xffaaa2),
        "codex" => Rgb::from_hex(0x9be6b4),
        "gemini" => Rgb::from_hex(0x9fc8ff),
        _ => TAG_BRANCH_TEXT,
    }
}

pub const fn status_color(status: TaskStatus) -> Rgb {
    match status {
        TaskStatus::Todo => Rgb::from_hex(0x64748b),
        TaskStatus::Progress => Rgb::from_hex(0x0064ff),
        TaskStatus::Pending => Rgb::from_hex(0xf59e0b),
        TaskStatus::Review => Rgb::from_hex(0x8b5cf6),
        TaskStatus::Done => Rgb::from_hex(0x10b981),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_token_matches_existing_design_contract() {
        assert_eq!(PRIMARY, Rgb::from_hex(0x0064ff));
        assert_eq!(NEUTRAL_BUTTON_SURFACE, Rgb::from_hex(0x202632));
    }

    #[test]
    fn status_order_matches_board_columns() {
        let statuses = TaskStatus::ALL.map(TaskStatus::as_str);

        assert_eq!(statuses, ["todo", "progress", "pending", "review", "done"]);
    }

    #[test]
    fn task_card_tag_colors_match_existing_semantic_tokens() {
        assert_eq!(priority_tag_color(TaskPriority::Low), PRIMARY);
        assert_eq!(
            priority_tag_color(TaskPriority::Medium),
            Rgb::from_hex(0xffd58a)
        );
        assert_eq!(
            priority_tag_color(TaskPriority::High),
            Rgb::from_hex(0xffaaa2)
        );
        assert_eq!(agent_tag_color("claude"), Rgb::from_hex(0xffaaa2));
        assert_eq!(agent_tag_color("codex"), Rgb::from_hex(0x9be6b4));
        assert_eq!(agent_tag_color("gemini"), Rgb::from_hex(0x9fc8ff));
        assert_eq!(agent_tag_color("opencode"), TAG_BRANCH_TEXT);
    }
}
