/** 프로젝트 색상 관련 유틸리티 */

const PRESET_COLORS = [
  "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD",
  "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];

/** 프로젝트명을 기반으로 결정론적 해시 색상을 계산한다 */
export function computeProjectColor(projectName: string): string {
  let hash = 0;
  for (let i = 0; i < projectName.length; i++) {
    hash = (hash * 31 + projectName.charCodeAt(i)) | 0;
  }
  return PRESET_COLORS[((hash % 8) + 8) % 8];
}
