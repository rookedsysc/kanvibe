/** 프로젝트 색상 관련 유틸리티 */

const PRESET_COLORS = [
  "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
  "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];

const TEXT_ON_LIGHT_BACKGROUND = "#111827";
const TEXT_ON_DARK_BACKGROUND = "#FFFFFF";

/** 프로젝트명을 기반으로 결정론적 해시 색상을 계산한다 */
export function computeProjectColor(projectName: string): string {
  let hash = 0;
  for (let i = 0; i < projectName.length; i++) {
    hash = (hash * 31 + projectName.charCodeAt(i)) | 0;
  }
  return PRESET_COLORS[((hash % 8) + 8) % 8];
}

/**
 * 프로젝트 색상 위에 올릴 글자색을 고른다.
 * 사용자가 어떤 색을 지정하든 이니셜이 읽히도록 배경 휘도에 따라 검정/흰색을 선택한다.
 * @param backgroundColor - `#RRGGBB` 형태의 배경색. 형식이 다르면 어두운 글자를 반환한다
 */
export function getReadableTextColor(backgroundColor: string): string {
  const rgb = parseHexColor(backgroundColor);
  if (!rgb) {
    return TEXT_ON_LIGHT_BACKGROUND;
  }

  const backgroundLuminance = rgb.reduce(
    (total, channel, index) => total + toLinearChannel(channel) * SRGB_LUMINANCE_WEIGHTS[index],
    0,
  );

  return getContrastRatio(backgroundLuminance, DARK_TEXT_LUMINANCE)
    >= getContrastRatio(backgroundLuminance, WHITE_TEXT_LUMINANCE)
    ? TEXT_ON_LIGHT_BACKGROUND
    : TEXT_ON_DARK_BACKGROUND;
}

const SRGB_LUMINANCE_WEIGHTS = [0.2126, 0.7152, 0.0722];
const DARK_TEXT_LUMINANCE = 0.0092;
const WHITE_TEXT_LUMINANCE = 1;

/** WCAG 대비율 `(밝은 쪽 + 0.05) / (어두운 쪽 + 0.05)` */
function getContrastRatio(luminanceA: number, luminanceB: number): number {
  const lighter = Math.max(luminanceA, luminanceB);
  const darker = Math.min(luminanceA, luminanceB);
  return (lighter + 0.05) / (darker + 0.05);
}

function parseHexColor(color: string): [number, number, number] | null {
  const matched = color.trim().match(/^#([0-9a-fA-F]{6})$/);
  if (!matched) {
    return null;
  }

  const value = Number.parseInt(matched[1], 16);
  return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

function toLinearChannel(channel: number): number {
  const normalized = channel / 255;
  return normalized <= 0.03928
    ? normalized / 12.92
    : ((normalized + 0.055) / 1.055) ** 2.4;
}
