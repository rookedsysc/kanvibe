//! Herdr endpoint generation 1이 쓰는 bincode 표준 프레이밍. 프레임은 4바이트 LE 길이 뒤에 본문이 오고,
//! 문자열은 varint 길이(251 미만은 한 바이트, 251·252·253은 뒤따르는 2·4·8바이트 LE) 뒤에 UTF-8 바이트가 온다.

use std::mem;

pub(super) const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

const FRAME_HEADER_BYTES: usize = 4;

pub(super) fn encode_string(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut encoded = Vec::with_capacity(bytes.len() + 5);
    if bytes.len() < 251 {
        encoded.push(bytes.len() as u8);
    } else if bytes.len() <= 0xffff {
        encoded.push(251);
        encoded.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    } else {
        encoded.push(252);
        encoded.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    }
    encoded.extend_from_slice(bytes);
    encoded
}

/// `position`의 문자열을 읽고 문자열 다음 위치를 함께 준다. 잘못된 UTF-8은 Node `toString("utf8")`처럼 대체 문자로 바꾼다
pub(super) fn read_string(payload: &[u8], position: usize) -> Result<(String, usize), String> {
    let Some(&marker) = payload.get(position) else {
        return Err("Herdr 문자열이 끊겼습니다.".to_string());
    };
    let mut position = position + 1;
    let mut length = usize::from(marker);
    if marker >= 251 {
        let width = match marker {
            251 => 2,
            252 => 4,
            253 => 8,
            _ => 0,
        };
        if width == 0 || position + width > payload.len() {
            return Err("Herdr 문자열 길이가 올바르지 않습니다.".to_string());
        }
        let size = read_little_endian(&payload[position..position + width]);
        if size > MAX_FRAME_BYTES as u64 {
            return Err("Herdr 문자열 크기 초과".to_string());
        }
        length = size as usize;
        position += width;
    }
    if position + length > payload.len() {
        return Err("Herdr 문자열이 끊겼습니다.".to_string());
    }
    let text = String::from_utf8_lossy(&payload[position..position + length]).into_owned();
    Ok((text, position + length))
}

/// surface 크기에 쓰는 부호 없는 varint. 8바이트 표기는 받지 않는다
pub(super) fn read_unsigned(payload: &[u8], position: usize) -> Result<(u32, usize), String> {
    let marker = payload.get(position).copied();
    let position = position + 1;
    match marker {
        Some(marker) if marker < 251 => Ok((u32::from(marker), position)),
        Some(251) if position + 2 <= payload.len() => Ok((
            read_little_endian(&payload[position..position + 2]) as u32,
            position + 2,
        )),
        Some(252) if position + 4 <= payload.len() => Ok((
            read_little_endian(&payload[position..position + 4]) as u32,
            position + 4,
        )),
        _ => Err("Herdr surface 크기가 올바르지 않습니다.".to_string()),
    }
}

fn read_little_endian(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0, |value, byte| (value << 8) | u64::from(*byte))
}

pub(super) fn frame(payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
    framed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    framed.extend_from_slice(payload);
    framed
}

/// 소켓에서 받은 조각을 이어 붙여 완성된 프레임 본문을 하나씩 넘긴다.
/// 본문은 받은 조각을 그대로 덧붙여 모으므로 복사량은 입력 크기에 비례한다.
#[derive(Default)]
pub(super) struct FrameReader {
    header: [u8; FRAME_HEADER_BYTES],
    header_bytes: usize,
    payload_length: usize,
    payload: Vec<u8>,
}

impl FrameReader {
    /// 조각 하나를 처리한다. 프레임 크기가 잘못됐거나 `receive`가 실패하면 그 오류로 멈춘다
    pub(super) fn push(
        &mut self,
        chunk: &[u8],
        mut receive: impl FnMut(Vec<u8>) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut offset = 0;
        while offset < chunk.len() {
            if self.header_bytes < FRAME_HEADER_BYTES {
                offset += self.read_header(&chunk[offset..])?;
                if self.header_bytes < FRAME_HEADER_BYTES {
                    break;
                }
            }
            let count = (self.payload_length - self.payload.len()).min(chunk.len() - offset);
            self.payload
                .extend_from_slice(&chunk[offset..offset + count]);
            offset += count;
            if self.payload.len() < self.payload_length {
                break;
            }
            self.header_bytes = 0;
            receive(mem::take(&mut self.payload))?;
        }
        Ok(())
    }

    /// 헤더에 모자란 만큼 읽고 읽은 바이트 수를 준다. 헤더가 다 차면 본문 길이를 검사한다
    fn read_header(&mut self, chunk: &[u8]) -> Result<usize, String> {
        let count = (FRAME_HEADER_BYTES - self.header_bytes).min(chunk.len());
        self.header[self.header_bytes..self.header_bytes + count].copy_from_slice(&chunk[..count]);
        self.header_bytes += count;
        if self.header_bytes == FRAME_HEADER_BYTES {
            self.payload_length = u32::from_le_bytes(self.header) as usize;
            if self.payload_length == 0 || self.payload_length > MAX_FRAME_BYTES {
                return Err("Herdr 프레임 크기가 올바르지 않습니다.".to_string());
            }
        }
        Ok(count)
    }
}
