//! packet 다이어그램(packet/parser.ts·renderer.ts). 비트 구간을 한 줄 32비트 칸에 늘어놓는다.
//!
//! 구간은 `시작-끝: "이름"`, `시작: "이름"`, `+비트수: "이름"`으로 쓰고 앞 구간에 이어져야 한다. 줄 끝을 넘는
//! 구간은 줄마다 잘라 같은 이름으로 다시 쓴다. 칸마다 사각형과 가운데 이름, 위쪽에 시작·끝 비트 번호를 둔다.

use serde_json::Value;

use crate::directive::{diagram_body, read_config};
use crate::scene::{Color, Point, Scene, StrokeStyle, TextItem, polygon};
use crate::text::{FontBook, FontWeight};
use crate::theme::Theme;
use crate::theme_variables::ThemeVariables;
use crate::{DiagramError, RenderContext};

/// Noto Sans x-height(1em 기준). `dominant-baseline: middle`은 기준선을 x-height 절반만큼 내린다
const X_HEIGHT_RATIO: f64 = 0.536;
const LABEL_FONT_SIZE: f64 = 12.0;
const BYTE_FONT_SIZE: f64 = 10.0;
const TITLE_FONT_SIZE: f64 = 14.0;
/// parser.ts maxPacketSize: 줄 수 상한
const MAX_PACKET_WORDS: usize = 10_000;

/// defaultConfig.packet. showBits가 참이면 getConfig가 paddingY에 10을 더한다
#[derive(Clone, Copy, Debug, PartialEq)]
struct PacketConfig {
    row_height: f64,
    bit_width: f64,
    bits_per_row: u64,
    is_showing_bits: bool,
    padding_x: f64,
    padding_y: f64,
}

impl PacketConfig {
    fn from_tree(section: Option<&serde_json::Map<String, Value>>) -> Self {
        let number = |key: &str, default: f64| {
            section
                .and_then(|map| map.get(key))
                .and_then(Value::as_f64)
                .unwrap_or(default)
        };
        let is_showing_bits = section
            .and_then(|map| map.get("showBits"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let padding_y = number("paddingY", 5.0);
        Self {
            row_height: number("rowHeight", 32.0),
            bit_width: number("bitWidth", 32.0),
            bits_per_row: number("bitsPerRow", 32.0).max(1.0) as u64,
            is_showing_bits,
            padding_x: number("paddingX", 5.0),
            padding_y: if is_showing_bits {
                padding_y + 10.0
            } else {
                padding_y
            },
        }
    }
}

/// 한 줄 안에 든 구간
#[derive(Clone, Debug, PartialEq)]
struct PacketBlock {
    start: u64,
    end: u64,
    label: String,
}

/// 줄 단위로 나눈 구간과 제목
#[derive(Clone, Debug, Default, PartialEq)]
struct Packet {
    words: Vec<Vec<PacketBlock>>,
    title: Option<String>,
}

/// 문장에서 읽은 구간: 시작이 없으면 앞 구간 바로 뒤다
struct BlockStatement {
    start: Option<u64>,
    end: Option<u64>,
    bits: Option<u64>,
    label: String,
}

fn syntax(message: String) -> DiagramError {
    DiagramError::Syntax(message)
}

fn parse(definition: &str, bits_per_row: u64) -> Result<Packet, DiagramError> {
    let body = diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines.next().unwrap_or_default();
    if header != "packet" && header != "packet-beta" {
        return Err(syntax(format!("packet 머리줄: {header}")));
    }
    let mut packet = Packet::default();
    let mut statements = Vec::new();
    for line in lines {
        if let Some(title) = line.strip_prefix("title") {
            packet.title = Some(title.trim().to_string());
        } else if !line.starts_with("accTitle") && !line.starts_with("accDescr") {
            statements.push(parse_block(line)?);
        }
    }
    packet.words = split_into_words(statements, bits_per_row)?;
    Ok(packet)
}

/// `0-15: "이름"`·`106: "이름"`·`+8: "이름"`
fn parse_block(line: &str) -> Result<BlockStatement, DiagramError> {
    let invalid = || syntax(format!("packet 구간 형식: {line}"));
    let (range, label) = line.split_once(':').ok_or_else(invalid)?;
    let label = unquote(label.trim()).ok_or_else(invalid)?;
    let number = |text: &str| text.trim().parse::<u64>().map_err(|_| invalid());
    let range = range.trim();
    if let Some(bits) = range.strip_prefix('+') {
        return Ok(BlockStatement {
            start: None,
            end: None,
            bits: Some(number(bits)?),
            label,
        });
    }
    let (start, end) = match range.split_once('-') {
        Some((start, end)) => (number(start)?, Some(number(end)?)),
        None => (number(range)?, None),
    };
    Ok(BlockStatement {
        start: Some(start),
        end,
        bits: None,
        label,
    })
}

/// 큰따옴표·작은따옴표 글자(역슬래시 다음 글자는 그대로 둔다)
fn unquote(text: &str) -> Option<String> {
    let quote = text.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let inner = text.strip_prefix(quote)?.strip_suffix(quote)?;
    Some(inner.replace(&format!("\\{quote}"), &quote.to_string()))
}

/// populate: 구간이 이어지는지 확인하고 줄 경계에서 잘라 줄마다 모은다
fn split_into_words(
    statements: Vec<BlockStatement>,
    bits_per_row: u64,
) -> Result<Vec<Vec<PacketBlock>>, DiagramError> {
    let mut words: Vec<Vec<PacketBlock>> = Vec::new();
    let mut word: Vec<PacketBlock> = Vec::new();
    let mut next_bit = 0;
    let mut row = 1;
    for statement in statements {
        let mut block = resolve_block(statement, next_bit)?;
        next_bit = block.end + 1;
        while words.len() < MAX_PACKET_WORDS {
            let row_end = row * bits_per_row;
            let rest = (block.end + 1 > row_end).then(|| PacketBlock {
                start: row_end,
                end: block.end,
                label: block.label.clone(),
            });
            block.end = block.end.min(row_end - 1);
            let is_row_full = block.end + 1 == row_end;
            word.push(block);
            if is_row_full {
                words.push(std::mem::take(&mut word));
                row += 1;
            }
            let Some(rest) = rest else {
                break;
            };
            block = rest;
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn resolve_block(statement: BlockStatement, next_bit: u64) -> Result<PacketBlock, DiagramError> {
    if let (Some(start), Some(end)) = (statement.start, statement.end)
        && end < start
    {
        return Err(syntax(format!(
            "packet 구간 {start} - {end}: 끝이 시작보다 작다"
        )));
    }
    let start = statement.start.unwrap_or(next_bit);
    if start != next_bit {
        return Err(syntax(format!(
            "packet 구간 {start}이 이어지지 않는다(다음 비트 {next_bit})"
        )));
    }
    if statement.bits == Some(0) {
        return Err(syntax(format!("packet 구간 {start}: 0비트")));
    }
    let end = statement
        .end
        .unwrap_or(start + statement.bits.unwrap_or(1) - 1);
    Ok(PacketBlock {
        start,
        end,
        label: statement.label,
    })
}

/// packet/styles.ts: `cleanAndMerge(기본값, themeVariables.packet)`. 기본값은 검정 글자·테두리와 `#efefef` 채움이고,
/// dark·forest 테마만 `packet` 묶음을 둔다
struct PacketColors {
    start_byte: Color,
    end_byte: Color,
    label: Color,
    title: Color,
    block_fill: Color,
    block_stroke: Color,
    block_stroke_width: f64,
}

impl PacketColors {
    fn defaults() -> Self {
        let black = Color::rgb(0, 0, 0);
        Self {
            start_byte: black,
            end_byte: black,
            label: black,
            title: black,
            block_fill: Color::rgb(0xEF, 0xEF, 0xEF),
            block_stroke: black,
            block_stroke_width: 1.0,
        }
    }

    /// 테마 변수가 비어 있으면(테스트용 기본값) 앱 테마의 `packet` 묶음 값을 쓴다
    fn read(theme: Theme, variables: &ThemeVariables) -> Self {
        let mut colors = Self::defaults();
        if variables.text("primaryColor").is_none() && theme == Theme::Dark {
            let text = Color::rgb(0xE0, 0xDF, 0xDF);
            colors = Self {
                start_byte: text,
                end_byte: text,
                label: text,
                title: text,
                block_fill: Color::rgb(0x33, 0x33, 0x33),
                block_stroke: text,
                ..colors
            };
        }
        let packet = variables.section("packet");
        packet.read_color("startByteColor", &mut colors.start_byte);
        packet.read_color("endByteColor", &mut colors.end_byte);
        packet.read_color("labelColor", &mut colors.label);
        packet.read_color("titleColor", &mut colors.title);
        packet.read_color("blockFillColor", &mut colors.block_fill);
        packet.read_color("blockStrokeColor", &mut colors.block_stroke);
        if let Some(width) = packet.css_length("blockStrokeWidth") {
            colors.block_stroke_width = width;
        }
        colors
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let tree = read_config(definition);
    let config = PacketConfig::from_tree(tree.section("packet"));
    let mut packet = parse(definition, config.bits_per_row)?;
    if packet.title.is_none() {
        packet.title = tree.title;
    }
    let colors = PacketColors::read(context.palette.theme, &context.theme_variables);
    let total_row_height = config.row_height + config.padding_y;
    let has_title = packet
        .title
        .as_deref()
        .is_some_and(|title| !title.is_empty());
    let height = total_row_height * (packet.words.len() as f64 + 1.0)
        - if has_title { 0.0 } else { config.row_height };
    let width = config.bit_width * config.bits_per_row as f64 + 2.0;
    let mut scene = Scene {
        width,
        height,
        items: Vec::new(),
        is_start_aligned: false,
    };
    let painter = WordPainter {
        fonts: context.fonts,
        config: &config,
        colors: &colors,
    };
    for (row, word) in packet.words.iter().enumerate() {
        painter.draw_word(&mut scene, word, row);
    }
    if let Some(title) = packet.title.filter(|title| !title.is_empty()) {
        let center = Point::new(width / 2.0, height - total_row_height / 2.0);
        let text = painter.anchored_text(
            title,
            center,
            (TITLE_FONT_SIZE, colors.title),
            TextAnchor::Middle,
        );
        scene.push_text(middle_baseline(text));
    }
    Ok(scene)
}

#[derive(Clone, Copy)]
enum TextAnchor {
    Start,
    Middle,
    End,
}

/// `dominant-baseline: middle` 글자의 기준선을 내린다
fn middle_baseline(mut text: TextItem) -> TextItem {
    text.baseline += X_HEIGHT_RATIO * text.font_size / 2.0;
    text
}

struct WordPainter<'a> {
    fonts: &'a FontBook,
    config: &'a PacketConfig,
    colors: &'a PacketColors,
}

impl WordPainter<'_> {
    /// drawWord: 구간마다 사각형, 가운데 이름, 시작·끝 비트 번호(한 비트면 가운데 번호 하나)
    fn draw_word(&self, scene: &mut Scene, word: &[PacketBlock], row: usize) {
        let config = self.config;
        let word_y = row as f64 * (config.row_height + config.padding_y) + config.padding_y;
        for block in word {
            let x = (block.start % config.bits_per_row) as f64 * config.bit_width + 1.0;
            let width = (block.end - block.start + 1) as f64 * config.bit_width - config.padding_x;
            let corners = [
                Point::new(x, word_y),
                Point::new(x + width, word_y),
                Point::new(x + width, word_y + config.row_height),
                Point::new(x, word_y + config.row_height),
            ];
            let stroke = StrokeStyle {
                color: self.colors.block_stroke,
                width: self.colors.block_stroke_width,
                dash: Vec::new(),
            };
            scene.push_path(
                polygon(&corners),
                Some(self.colors.block_fill),
                Some(stroke),
            );
            let center = Point::new(x + width / 2.0, word_y + config.row_height / 2.0);
            let label = self.anchored_text(
                block.label.clone(),
                center,
                (LABEL_FONT_SIZE, self.colors.label),
                TextAnchor::Middle,
            );
            scene.push_text(middle_baseline(label));
            if config.is_showing_bits {
                self.draw_bit_numbers(scene, block, x, width, word_y - 2.0);
            }
        }
    }

    fn draw_bit_numbers(
        &self,
        scene: &mut Scene,
        block: &PacketBlock,
        x: f64,
        width: f64,
        baseline: f64,
    ) {
        if block.start == block.end {
            let center = Point::new(x + width / 2.0, baseline);
            let text = block.start.to_string();
            let byte = (BYTE_FONT_SIZE, self.colors.start_byte);
            scene.push_text(self.anchored_text(text, center, byte, TextAnchor::Middle));
            return;
        }
        let start = block.start.to_string();
        let start_point = Point::new(x, baseline);
        let start_byte = (BYTE_FONT_SIZE, self.colors.start_byte);
        scene.push_text(self.anchored_text(start, start_point, start_byte, TextAnchor::Start));
        let end = block.end.to_string();
        let end_point = Point::new(x + width, baseline);
        let end_byte = (BYTE_FONT_SIZE, self.colors.end_byte);
        scene.push_text(self.anchored_text(end, end_point, end_byte, TextAnchor::End));
    }

    /// `text-anchor`에 맞춰 글자 왼쪽을 정한다. `point.y`는 기준선이다
    fn anchored_text(
        &self,
        text: String,
        point: Point,
        (font_size, color): (f64, Color),
        anchor: TextAnchor,
    ) -> TextItem {
        let advance = self
            .fonts
            .shape_line(&text, font_size, FontWeight::Regular)
            .width;
        let x = match anchor {
            TextAnchor::Start => point.x,
            TextAnchor::Middle => point.x - advance / 2.0,
            TextAnchor::End => point.x - advance,
        };
        TextItem {
            text,
            x,
            baseline: point.y,
            font_size,
            weight: FontWeight::Regular,
            color,
            rotation: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(start: u64, end: u64, label: &str) -> PacketBlock {
        PacketBlock {
            start,
            end,
            label: label.to_string(),
        }
    }

    #[test]
    fn 상대_구간은_앞_구간에_이어_붙고_줄_경계에서_잘린다() {
        let packet = parse(
            "packet\n+16: \"Source\"\n+16: 'Dest'\n+40: \"Seq\"\n+8: \"Flags\"\n80: \"A\"",
            32,
        )
        .unwrap();
        assert_eq!(packet.words.len(), 3);
        assert_eq!(
            packet.words[0],
            vec![block(0, 15, "Source"), block(16, 31, "Dest")]
        );
        assert_eq!(packet.words[1], vec![block(32, 63, "Seq")]);
        assert_eq!(
            packet.words[2],
            vec![
                block(64, 71, "Seq"),
                block(72, 79, "Flags"),
                block(80, 80, "A")
            ]
        );
    }

    #[test]
    fn 이어지지_않거나_0비트인_구간은_문법_오류다() {
        assert!(parse("packet-beta\n0-15: \"a\"\n17-20: \"b\"", 32).is_err());
        assert!(parse("packet\n+0: \"a\"", 32).is_err());
        assert!(parse("packet\n5-3: \"a\"", 32).is_err());
    }

    #[test]
    fn 제목이_없으면_한_줄_높이를_뺀다() {
        // packet-relative 기준 SVG: viewBox 0 0 1026 156, 첫 칸 x=1 y=15 폭 507
        let config = PacketConfig::from_tree(None);
        assert_eq!(config.padding_y, 15.0);
        let rows = 3.0;
        let height = (config.row_height + config.padding_y) * (rows + 1.0) - config.row_height;
        assert_eq!(height, 156.0);
        assert_eq!(config.bit_width * config.bits_per_row as f64 + 2.0, 1026.0);
    }
}
