/** 입력란의 첫 줄. 표시 기호(셸 모드에서는 !)와 줄바꿈 없는 공백 뒤에 글이 온다 */
const COMPOSER_FIRST_ROW = /^([❯!])[  ](.*)$/;
/** 입력란에서 이어지는 줄의 들여쓰기 */
const COMPOSER_CONTINUATION_INDENT = "  ";
/** 입력란 위쪽의 경계선 */
const COMPOSER_BORDER = /^─{10,}\s*$/;
/** 붙여넣은 이미지 하나를 대신하는 표시 `[Image #N]`. 공백을 뺀 글에서 찾는다 */
const IMAGE_PLACEHOLDER = /^\[Image#\d+\]/;
/** 붙여넣은 글을 접어 보여 주는 표시 `[Pasted text #N +M lines]`. 여러 줄이면 첫 줄 뒤의 줄 수가 붙는다. 공백을 뺀 글에서 찾는다 */
const FOLDED_PASTE = /^\[Pastedtext#\d+(?:\+(\d+)lines)?\]$/;
/** 한 줄짜리 글을 접기 시작하는 길이의 하한. 700자는 그대로 보였고 1,100자는 접혔다 */
const FOLDED_SINGLE_LINE_MIN_LENGTH = 700;
/** 여러 줄 글을 접기 시작하는 줄 수의 하한. 3줄은 그대로 보였고 4줄부터 접혔다 */
const FOLDED_MIN_LINE_COUNT = 4;

const withoutWhitespace = (text: string) => text.replace(/\s/g, "");

/**
 * Claude Code 입력란에 지금 들어 있는 글을 화면에서 읽는다. 커서가 입력란의 마지막 줄에 있을 때만 읽고, 아니면 null이다.
 *
 * 입력란은 경계선 바로 아래에서 시작한다. 대화 본문에 남은 지난 메시지나 선택 화면의 선택지도 같은 기호로 시작하므로,
 * 첫 줄 위가 경계선인지를 함께 본다. Claude Code 2.1.326의 실제 화면으로 확인한 배치다.
 */
export function readClaudeComposerText(lines: string[], cursorRow: number): string | null {
  // 이어지는 줄은 두 칸 들여쓰고, 문단 사이나 글 끝의 빈 줄은 화면에서도 빈 줄이다
  const isContinuationRow = (line: string) => !line.trim() || line.startsWith(COMPOSER_CONTINUATION_INDENT);
  let firstRow = cursorRow;
  while (firstRow > 0 && !COMPOSER_FIRST_ROW.test(lines[firstRow] ?? "") && isContinuationRow(lines[firstRow] ?? "")) firstRow--;
  const first = COMPOSER_FIRST_ROW.exec(lines[firstRow] ?? "");
  if (!first || firstRow < 1 || !COMPOSER_BORDER.test(lines[firstRow - 1])) return null;
  const continuationRows = lines.slice(firstRow + 1, cursorRow + 1).map((line) => line.slice(COMPOSER_CONTINUATION_INDENT.length));
  return [`${first[1] === "!" ? "!" : ""}${first[2]}`, ...continuationRows].join("\n").trimEnd();
}

/**
 * 입력란의 글이 방금 붙여넣은 글 그대로인지 확인한다.
 *
 * Claude는 긴 글을 화면 너비에 맞춰 여러 줄로 그리므로 공백과 줄바꿈은 견주지 않는다. 4줄 이상이거나 아주 긴 글은
 * `[Pasted text #N +M lines]`로 접어 보여 주고 이미지는 `[Image #N]`으로 바꾸므로, 그 표시가 붙여넣은 글의 모양과 맞는지를 본다.
 */
export function isClaudeComposerShowingPaste(composerText: string, pastedText: string, imageCount: number): boolean {
  let shown = withoutWhitespace(composerText);
  for (let image = 0; image < imageCount; image++) {
    const placeholder = IMAGE_PLACEHOLDER.exec(shown);
    if (!placeholder) return false;
    shown = shown.slice(placeholder[0].length);
  }
  if (IMAGE_PLACEHOLDER.test(shown)) return false;
  if (shown === withoutWhitespace(pastedText)) return true;
  const folded = FOLDED_PASTE.exec(shown);
  if (!folded) return false;
  const lineCount = pastedText.split("\n").length;
  if (folded[1] === undefined) return lineCount === 1 && pastedText.length >= FOLDED_SINGLE_LINE_MIN_LENGTH;
  return Number(folded[1]) === lineCount - 1 && (lineCount >= FOLDED_MIN_LINE_COUNT || pastedText.length >= FOLDED_SINGLE_LINE_MIN_LENGTH);
}
