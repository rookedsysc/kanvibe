//! 원격 세션 이미지 붙여넣기(TS `Terminal.tsx` `sendImageToRemoteTerminal`). 원격 CLI는 로컬 클립보드를 읽지 못하므로
//! 이미지를 scp로 보내고 원격 경로만 글로 붙인다. 브리지가 data URL을 받으므로 여기서 만든다.

use gpui_kit::{ClipboardEntry, ClipboardItem, Image};

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 클립보드의 첫 이미지
pub fn clipboard_image(item: &ClipboardItem) -> Option<Image> {
    item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image.clone()),
        _ => None,
    })
}

/// TS `readClipboardImage`가 돌려주던 `data:image/png;base64,...`
pub fn image_data_url(image: &Image) -> String {
    format!("data:image/png;base64,{}", encode_base64(&image.bytes))
}

fn encode_base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | u32::from(*byte) << (16 - 8 * index)
        });
        let symbol_count = chunk.len() + 1;
        for index in 0..4 {
            match index < symbol_count {
                true => {
                    let sextet = (value >> (18 - 6 * index)) & 0x3f;
                    encoded.push(char::from(BASE64_ALPHABET[sextet as usize]));
                }
                false => encoded.push('='),
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_with_padding() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(&[0xff, 0xfe, 0xfd, 0x00]), "//79AA==");
    }
}
