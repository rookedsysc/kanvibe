//! Monaco 찾기 위젯이 쓰는 아이콘 글꼴 codicon. 파일은 TS 앱이 Monaco로 불러 쓰던 `codicon.ttf`(monaco-editor 0.55.1) 그대로이고
//! 라이선스는 CC BY 4.0이다(`assets/LICENSE-codicon-CC-BY-4.0.txt`, 출처·변경 표시는 `assets/NOTICE-codicon.txt`).
//!
//! GPUI는 'm' 글리프가 없는 얼굴을 글꼴로 올리지 않는다(글자 폭을 'm'으로 재기 때문). codicon에는 'm'이 없으므로,
//! 등록할 때만 메모리 사본의 cmap에 'm' → 기존 글리프 하나를 더한다. 글리프 윤곽선과 다른 표는 바꾸지 않는다.

use std::borrow::Cow;

use gpui_kit::{App, Font, FontFeatures, FontStyle, FontWeight};

const CODICON_FONT_FILE: &[u8] = include_bytes!("assets/codicon.ttf");
/// `codicon.css`의 `@font-face { font-family: "codicon" }`
const CODICON_FAMILY: &str = "codicon";
const LETTER_M: u32 = 'm' as u32;
/// OpenType 명세의 `head.checkSumAdjustment` 기준값
const CHECKSUM_MAGIC: u32 = 0xB1B0_AFBA;

/// 찾기 위젯 아이콘(Monaco `codiconsLibrary`의 글자 번호)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codicon {
    CaseSensitive,
    WholeWord,
    Regex,
    PreserveCase,
    ArrowUp,
    ArrowDown,
    Selection,
    Close,
    Replace,
    ReplaceAll,
    ChevronRight,
    ChevronDown,
}

impl Codicon {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::CaseSensitive => "\u{EAB1}",
            Self::WholeWord => "\u{EB7E}",
            Self::Regex => "\u{EB38}",
            Self::PreserveCase => "\u{EB2E}",
            Self::ArrowUp => "\u{EAA1}",
            Self::ArrowDown => "\u{EA9A}",
            Self::Selection => "\u{EB85}",
            Self::Close => "\u{EA76}",
            Self::Replace => "\u{EB3D}",
            Self::ReplaceAll => "\u{EB3C}",
            Self::ChevronRight => "\u{EAB6}",
            Self::ChevronDown => "\u{EAB4}",
        }
    }
}

pub fn codicon_font() -> Font {
    Font {
        family: CODICON_FAMILY.into(),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

/// 앱 글자 시스템에 codicon이 없으면 올린다. 찾기 위젯을 처음 열 때 부른다
pub fn ensure_codicon_font(cx: &App) {
    let text_system = cx.text_system();
    let font = codicon_font();
    let resolved = text_system.resolve_font(&font);
    let is_loaded = text_system
        .get_font_for_id(resolved)
        .is_some_and(|loaded| loaded.family == font.family);
    if is_loaded {
        return;
    }
    if let Some(patched) = with_letter_m_mapped(CODICON_FONT_FILE) {
        let _ = text_system.add_fonts(vec![Cow::Owned(patched)]);
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

/// 표 목록에서 `tag` 표의 기록 위치(표 목록 안 바이트 위치)
fn table_record(font: &[u8], tag: &[u8; 4]) -> Option<usize> {
    let table_count = usize::from(read_u16(font, 4)?);
    (0..table_count)
        .map(|index| 12 + 16 * index)
        .find(|record| font.get(*record..record + 4) == Some(tag.as_slice()))
}

/// cmap의 (3, 10) 형식 12 하위 표에서 (시작 글자, 끝 글자, 시작 글리프) 묶음을 읽는다
fn unicode_groups(font: &[u8], cmap_offset: usize) -> Option<Vec<(u32, u32, u32)>> {
    let subtable_count = usize::from(read_u16(font, cmap_offset + 2)?);
    let subtable = (0..subtable_count).find_map(|index| {
        let record = cmap_offset + 4 + 8 * index;
        if read_u16(font, record)? != 3 || read_u16(font, record + 2)? != 10 {
            return None;
        }
        read_u32(font, record + 4).map(|offset| cmap_offset + offset as usize)
    })?;
    if read_u16(font, subtable)? != 12 {
        return None;
    }
    let group_count = read_u32(font, subtable + 12)? as usize;
    (0..group_count)
        .map(|index| {
            let group = subtable + 16 + 12 * index;
            Some((
                read_u32(font, group)?,
                read_u32(font, group + 4)?,
                read_u32(font, group + 8)?,
            ))
        })
        .collect()
}

/// (3, 10) 형식 12 하위 표 하나만 든 cmap 표
fn build_cmap(groups: &[(u32, u32, u32)]) -> Vec<u8> {
    let subtable_length = 16 + 12 * groups.len() as u32;
    let mut table = Vec::with_capacity(12 + subtable_length as usize);
    table.extend_from_slice(&[0, 0, 0, 1, 0, 3, 0, 10]);
    table.extend_from_slice(&12u32.to_be_bytes());
    table.extend_from_slice(&[0, 12, 0, 0]);
    table.extend_from_slice(&subtable_length.to_be_bytes());
    table.extend_from_slice(&0u32.to_be_bytes());
    table.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (start, end, glyph) in groups {
        for value in [start, end, glyph] {
            table.extend_from_slice(&value.to_be_bytes());
        }
    }
    table
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn pad_to_four_bytes(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(4), 0);
}

/// 원본 뒤에 'm'을 더한 cmap을 붙이고 표 목록이 그 cmap을 가리키게 한 사본. 다른 표의 위치·바이트는 그대로다
fn with_letter_m_mapped(font: &[u8]) -> Option<Vec<u8>> {
    let cmap_record = table_record(font, b"cmap")?;
    let mut groups = unicode_groups(font, read_u32(font, cmap_record + 8)? as usize)?;
    if groups
        .iter()
        .any(|(start, end, _)| (*start..=*end).contains(&LETTER_M))
    {
        return Some(font.to_vec());
    }
    let existing_glyph = groups.first()?.2;
    groups.push((LETTER_M, LETTER_M, existing_glyph));
    groups.sort_unstable();
    let cmap = build_cmap(&groups);
    let mut patched = font.to_vec();
    pad_to_four_bytes(&mut patched);
    let cmap_offset = patched.len() as u32;
    patched.extend_from_slice(&cmap);
    pad_to_four_bytes(&mut patched);
    write_u32(&mut patched, cmap_record + 4, checksum(&cmap));
    write_u32(&mut patched, cmap_record + 8, cmap_offset);
    write_u32(&mut patched, cmap_record + 12, cmap.len() as u32);
    let head_offset = read_u32(&patched, table_record(&patched, b"head")? + 8)? as usize;
    write_u32(&mut patched, head_offset + 8, 0);
    let adjustment = CHECKSUM_MAGIC.wrapping_sub(checksum(&patched));
    write_u32(&mut patched, head_offset + 8, adjustment);
    Some(patched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph_for(font: &[u8], character: u32) -> Option<u32> {
        let cmap_offset = read_u32(font, table_record(font, b"cmap")? + 8)? as usize;
        unicode_groups(font, cmap_offset)?
            .into_iter()
            .find(|(start, end, _)| (*start..=*end).contains(&character))
            .map(|(start, _, glyph)| glyph + character - start)
    }

    #[test]
    fn patched_font_maps_letter_m_and_keeps_every_find_widget_icon() {
        assert_eq!(glyph_for(CODICON_FONT_FILE, LETTER_M), None);

        let patched = with_letter_m_mapped(CODICON_FONT_FILE).unwrap();

        assert!(glyph_for(&patched, LETTER_M).is_some_and(|glyph| glyph != 0));
        for icon in [Codicon::CaseSensitive, Codicon::Regex, Codicon::ReplaceAll] {
            let character = icon.glyph().chars().next().unwrap() as u32;
            assert_eq!(
                glyph_for(&patched, character),
                glyph_for(CODICON_FONT_FILE, character)
            );
            assert!(glyph_for(&patched, character).is_some());
        }
        assert_eq!(checksum(&patched), CHECKSUM_MAGIC);
    }
}
