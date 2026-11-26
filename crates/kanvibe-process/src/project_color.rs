//! 프로젝트 색상 계산과 그 위에 올릴 글자색(TS `projectColor.ts`).

const PRESET_COLORS: [&str; 8] = [
    "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD", "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];

const TEXT_ON_LIGHT_BACKGROUND: &str = "#111827";
const TEXT_ON_DARK_BACKGROUND: &str = "#FFFFFF";

const SRGB_LUMINANCE_WEIGHTS: [f64; 3] = [0.2126, 0.7152, 0.0722];
const DARK_TEXT_LUMINANCE: f64 = 0.0092;
const WHITE_TEXT_LUMINANCE: f64 = 1.0;

/// 프로젝트명으로 결정론적 해시 색상을 고른다. JS처럼 UTF-16 단위를 32비트 정수로 접는다
pub fn compute_project_color(project_name: &str) -> &'static str {
    let hash = project_name.encode_utf16().fold(0_i32, |hash, code_unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(code_unit))
    });
    PRESET_COLORS[hash.rem_euclid(8) as usize]
}

fn parse_hex_color(color: &str) -> Option<[u8; 3]> {
    let hex_digits = color.trim().strip_prefix('#')?;
    if hex_digits.len() != 6 || !hex_digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex_digits, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

fn to_linear_channel(channel: u8) -> f64 {
    let normalized = f64::from(channel) / 255.0;
    if normalized <= 0.03928 {
        normalized / 12.92
    } else {
        ((normalized + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG 대비율 `(밝은 쪽 + 0.05) / (어두운 쪽 + 0.05)`
fn contrast_ratio(luminance_a: f64, luminance_b: f64) -> f64 {
    let lighter = luminance_a.max(luminance_b);
    let darker = luminance_a.min(luminance_b);
    (lighter + 0.05) / (darker + 0.05)
}

/// 사용자가 어떤 색을 지정하든 이니셜이 읽히도록 배경 휘도에 따라 검정·흰색 글자를 고른다.
/// `#RRGGBB`가 아니면 어두운 글자를 돌려준다.
pub fn readable_text_color(background_color: &str) -> &'static str {
    let Some(rgb) = parse_hex_color(background_color) else {
        return TEXT_ON_LIGHT_BACKGROUND;
    };
    let background_luminance: f64 = rgb
        .iter()
        .zip(SRGB_LUMINANCE_WEIGHTS)
        .map(|(channel, weight)| to_linear_channel(*channel) * weight)
        .sum();
    if contrast_ratio(background_luminance, DARK_TEXT_LUMINANCE)
        >= contrast_ratio(background_luminance, WHITE_TEXT_LUMINANCE)
    {
        TEXT_ON_LIGHT_BACKGROUND
    } else {
        TEXT_ON_DARK_BACKGROUND
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_hex_color(color: &str) -> bool {
        parse_hex_color(color).is_some() && !color.starts_with(' ')
    }

    #[test]
    fn returns_a_hex_color() {
        assert!(is_hex_color(compute_project_color("kanvibe")));
    }

    #[test]
    fn same_name_gives_the_same_color() {
        assert_eq!(
            compute_project_color("kanvibe"),
            compute_project_color("kanvibe")
        );
    }

    #[test]
    fn different_names_spread_over_several_colors() {
        let mut colors: Vec<&str> = ["kanvibe", "my-app", "backend", "frontend", "docs"]
            .iter()
            .map(|name| compute_project_color(name))
            .collect();
        colors.sort();
        colors.dedup();

        assert!(colors.len() >= 2);
    }

    #[test]
    fn returns_one_of_the_preset_colors() {
        assert!(PRESET_COLORS.contains(&compute_project_color("test-project")));
    }

    #[test]
    fn empty_name_still_gets_a_color() {
        assert!(is_hex_color(compute_project_color("")));
    }

    #[test]
    fn long_name_still_gets_a_color() {
        assert!(is_hex_color(compute_project_color(&"a".repeat(1000))));
    }

    #[test]
    fn single_character_name_gets_a_preset_color() {
        assert!(PRESET_COLORS.contains(&compute_project_color("a")));
    }

    #[test]
    fn hash_matches_the_js_value() {
        // JS: "a" → 97 % 8 = 1, "kanvibe"는 32비트로 넘쳐 음수가 된 뒤 양수 나머지로 접힌다
        assert_eq!(compute_project_color("a"), "#93C5FD");
        assert_eq!(compute_project_color(""), "#F9A8D4");
    }

    #[test]
    fn pastel_presets_get_dark_text() {
        for color in PRESET_COLORS {
            assert_eq!(readable_text_color(color), "#111827");
        }
    }

    #[test]
    fn dark_backgrounds_get_white_text() {
        for color in ["#0064FF", "#000000", "#202632"] {
            assert_eq!(readable_text_color(color), "#FFFFFF");
        }
    }

    #[test]
    fn malformed_colors_get_dark_text() {
        for color in ["not-a-color", "#FFF", ""] {
            assert_eq!(readable_text_color(color), "#111827");
        }
    }

    #[test]
    fn surrounding_spaces_are_ignored() {
        assert_eq!(readable_text_color("  #000000  "), "#FFFFFF");
    }
}
