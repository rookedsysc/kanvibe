import type { IBufferLine } from "@xterm/headless";

/** 화면의 footer 배치를 실제 TUI에서 확인한 버전. 상태 조회는 표시 설정을 바꾸지 않는다. */
const THREAD_FOOTER_VERIFIED_VERSIONS = new Set(["0.169.0"]);
const COMPOSER_MARKER = "›";
const COMPOSER_TEXT_COLUMN = 2;
/** 입력란이 비어 있을 때 Codex가 그 자리에 보여 주는 안내 문구. 사용자가 같은 글을 직접 입력할 수도 있어 글자만으로는 빈 입력란임이 증명되지 않는다 */
const EMPTY_COMPOSER_HINT = "Ask Codex to do anything";
/** 줄 전체가 대화 ID 하나이거나, 그 뒤에 메인 대화임을 알리는 Codex의 표시만 붙은 footer */
const THREAD_FOOTER_ROW = /^ {2}([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(?: · Main \[default\])?$/;
/** 메인 대화의 빈 입력란 아래에서 실제로 확인한 단축키 안내 줄. side 대화나 연결이 끊긴 화면의 안내 줄은 이와 다르다 */
const MAIN_SHORTCUT_HINT_ROW = /^ {2}← for agents(?: · \? for shortcuts(?: · ⌃\/ for side)?)?(?: +⚠ \d+ warnings? · f2 to view)?$/;
/** 실제 Codex에서 확인한 ASCII 한 칸과 완성형 한글 두 칸만 접힘 계산에 쓴다. 결합 문자와 폭이 모호한 기호는 추정하지 않는다 */
const KNOWN_COLUMN_TEXT = /^[\x20-\x7e\uac00-\ud7a3]*$/;
/** 줄바꿈 규칙에 따라 낱말 안에서도 그 뒤로 줄이 나뉠 수 있는 글자까지를, 다음 줄로 넘어갈 수 있는 가장 짧은 조각으로 센다 */
const SHORTEST_NEXT_PIECE = /^[^ \-/|!?}]*[-/|!?}]?/;
/** 앞에 공백이 있어도 그 앞에서는 줄을 나누지 않는 글자 */
const NO_BREAK_BEFORE = /^[)\]}!?/,.:;]/;
/** 뒤에 공백이 있어도 그 뒤에서는 줄을 나누지 않는 여는 괄호 */
const NO_BREAK_AFTER = /[([{]$/;

export interface CodexDisplay {
  /** 입력란에 보이는 글. 입력란이 비어 있으면 Codex의 안내 문구가 그대로 담기며, 사용자가 직접 입력한 같은 글과 구분되지 않는다 */
  composerText: string;
  /**
   * footer 자리에 보이는 대화 ID. 잘린 ID, 다른 항목이 섞인 footer, footer 밖의 UUID는 담지 않는다.
   *
   * 화면의 글자만 읽은 값이다. 사용자가 입력란에 안내 문구·UUID·단축키 안내와 같은 글을 써서 같은 배치를 만들 수 있으므로, 이 값만으로 새 대화를 받아들이지 않는다.
   * 처음 보는 ID를 받아들이거나 표시 명령을 치기 전에, 호출자가 같은 캡처에서 얻은 Codex 고유의 빈 입력란 표시(안내 문구의 글자 스타일)로 입력란이 실제로 비어 있음을 확인해야 한다. 그 확인이 없으면 이미 연결한 대화의 ID와 같은지 견주는 데만 쓴다.
   */
  threadId: string | null;
}

/** Codex의 기본 안내문과 동적 제안은 모두 빈 입력란에서 dim으로 그려진다. 같은 글을 직접 쓴 입력은 이 조건을 충족하지 않는다 */
export function isCodexEmptyComposer(line: IBufferLine | undefined, cursorColumn: number): boolean {
  if (cursorColumn !== COMPOSER_TEXT_COLUMN || !line?.translateToString(true).trimEnd().startsWith("› ")) return false;
  let lastTextColumn = line.length - 1;
  while (lastTextColumn >= COMPOSER_TEXT_COLUMN && !line.getCell(lastTextColumn)?.getChars().trim()) lastTextColumn--;
  let hasText = false;
  for (let column = COMPOSER_TEXT_COLUMN; column <= lastTextColumn; column++) {
    const cell = line.getCell(column);
    if (!cell?.getChars()) continue;
    hasText = true;
    if (!cell.isDim()) return false;
  }
  return hasText;
}

export function isCodexThreadFooterSupported(versionOutput: string): boolean {
  const version = versionOutput.trim().match(/^codex-cli (\S+)$/)?.[1];
  return version !== undefined && THREAD_FOOTER_VERIFIED_VERSIONS.has(version);
}

function isBlankRow(line: string | undefined): boolean {
  return (line ?? "").trim() === "";
}

function isComposerContinuationRow(line: string | undefined): boolean {
  return isBlankRow(line) || line!.startsWith(" ".repeat(COMPOSER_TEXT_COLUMN));
}

/** 커서 줄에서 위로 올라가며 입력란의 첫 줄을 찾는다. 첫 줄이 화면 밖으로 밀렸으면 찾지 못한다 */
function findComposerFirstRow(viewportLines: string[], cursorRow: number): number | null {
  for (let row = cursorRow; row >= 0; row--) {
    if (viewportLines[row]?.startsWith(COMPOSER_MARKER)) return row;
    if (!isComposerContinuationRow(viewportLines[row])) return null;
  }
  return null;
}

function readFooterThreadId(line: string | undefined): string | null {
  return line?.trimEnd().match(THREAD_FOOTER_ROW)?.[1] ?? null;
}

/** 실제 빈 입력란의 idle 안내만 허용한다. 실행 중인 메시지 대기 안내는 자동 명령/새 입력을 허용하지 않는다 */
export function isCodexMainComposerReady(viewportLines: string[], cursorRow: number): boolean {
  const lastRow = viewportLines.findLastIndex((line) => !isBlankRow(line));
  // 초기 상태줄은 설정 항목과 pane 너비에 따라 여러 줄로 접힌다. 빈 입력란과 마지막 안내 줄 사이의 연속된 상태줄을 확인한다.
  return lastRow >= cursorRow + 3 && isBlankRow(viewportLines[cursorRow + 1])
    && viewportLines.slice(cursorRow + 2, lastRow).every((line) => line.startsWith("  ") && !isBlankRow(line))
    && MAIN_SHORTCUT_HINT_ROW.test(viewportLines[lastRow]?.trimEnd() ?? "");
}

/**
 * 커서가 놓인 Codex 입력란과 그 아래 footer를 같은 화면에서 읽는다.
 *
 * footer의 위치만으로는 대화 ID를 읽지 않는다. 안내 문구가 보이는 입력란에서는 입력란 한 줄, 빈 줄, footer, 확인된 단축키 안내 줄이 차례로 화면 끝까지 이어져야 하고, 그 밖의 입력란에서는 입력란 끝에서 빈 줄 하나를 건넌 footer가 화면의 마지막 내용 줄이어야 한다.
 * 이 배치는 대화 ID를 믿기 위한 필요조건일 뿐이다. 남은 조건은 `CodexDisplay.threadId`에 적혀 있다.
 * 커서가 입력란에 있지 않으면 null이다.
 */
export function readCodexDisplay(viewportLines: string[], cursorRow: number, nativeEmpty = false): CodexDisplay | null {
  const firstRow = findComposerFirstRow(viewportLines, cursorRow);
  if (firstRow === null) return null;
  const readComposerText = (lastRow: number) => viewportLines.slice(firstRow, lastRow + 1).map((line) => line.slice(COMPOSER_TEXT_COLUMN)).join("\n");
  const firstLine = viewportLines[firstRow].slice(COMPOSER_TEXT_COLUMN).trimEnd();
  const lastContentRow = viewportLines.findLastIndex((line) => !isBlankRow(line));

  if (nativeEmpty || firstLine === EMPTY_COMPOSER_HINT) {
    const hintRow = cursorRow + 3;
    const isEmptyComposerLayout = firstRow === cursorRow && isBlankRow(viewportLines[cursorRow + 1])
      && lastContentRow === hintRow && (isCodexMainComposerReady(viewportLines, cursorRow) || viewportLines[hintRow].trimEnd() === "  tab to queue message");
    return { composerText: nativeEmpty ? "" : readComposerText(cursorRow), threadId: isEmptyComposerLayout ? readFooterThreadId(viewportLines[cursorRow + 2]) : null };
  }

  const footerRow = lastContentRow;
  const isFooterBelowDraft = firstLine !== "" && footerRow - 2 >= cursorRow && isBlankRow(viewportLines[footerRow - 1])
    && viewportLines.slice(cursorRow + 1, footerRow - 1).every(isComposerContinuationRow);
  const threadId = isFooterBelowDraft ? readFooterThreadId(viewportLines[footerRow]) : null;
  return { composerText: readComposerText(threadId ? footerRow - 2 : cursorRow), threadId };
}

/**
 * 기대한 줄의 `foldAt` 자리에서 화면의 줄이 바뀐 것이 Codex가 줄을 접은 자리인지 입력란의 글 너비로 확인한다. 접으면서 지운 공백 수를 돌려주고, 접힌 자리로 확인되지 않으면 null이다.
 *
 * 줄은 다음 조각이 그 줄에 들어갈 수 없을 때만 접힌다. 150칸 ASCII와 80칸 한글 화면에서 관측한 낱말 접힘과, 다음 글자가 들어갈 수 없는 강제 분할만 인정한다.
 * 칸 수를 글자 수로 셀 수 없는 글과, 줄바꿈 규칙에 따라 나뉠 수 있는지 확실하지 않은 자리는 접힌 것으로 보지 않는다.
 */
function countSpacesDroppedAtFold(expectedLine: string, foldAt: number, row: string, textColumns: number): number | null {
  const rest = expectedLine.slice(foldAt);
  const spaces = rest.length - rest.replace(/^ +/, "").length;
  const nextPiece = rest.slice(spaces).match(SHORTEST_NEXT_PIECE)![0];
  if (!KNOWN_COLUMN_TEXT.test(row + nextPiece)) return null;
  const columns = (text: string) => [...text].reduce((count, character) => count + (character.charCodeAt(0) < 0x80 ? 1 : 2), 0);
  const rowColumns = columns(row);
  if (rowColumns > textColumns) return null;
  const couldNextPieceFit = rowColumns + spaces + columns(nextPiece) <= textColumns;
  if (spaces > 0) return !couldNextPieceFit && !NO_BREAK_AFTER.test(row) && !NO_BREAK_BEFORE.test(nextPiece) ? spaces : null;
  const isAfterHyphenInsideWord = /[A-Za-z0-9]-$/.test(row) && /^[A-Za-z]/.test(nextPiece);
  const isHangulForcedSplit = /[\uac00-\ud7a3]$/.test(row) && /^[\uac00-\ud7a3]/.test(nextPiece) && rowColumns + 2 > textColumns;
  return (isAfterHyphenInsideWord && !couldNextPieceFit) || rowColumns === textColumns || isHangulForcedSplit ? 0 : null;
}

/**
 * 화면의 입력란 줄들이 기대한 글을 그대로 보여 주는지 견준다.
 *
 * 화면의 한 줄은 기대한 글의 한 줄과 글자 그대로 같아야 한다. 터미널이 지우는 줄 끝 공백만 예외다.
 * `viewportColumns`로 같은 캡처의 화면 너비를 주면, Codex가 긴 줄을 접어 여러 줄로 그린 것을 그 너비로 확인해 한 줄로 잇는다. 너비 없이는 접힘을 추정하지 않으므로 줄바꿈과 공백이 서로 섞이지 않는다.
 */
export function isSameComposerText(composerText: string, expectedText: string, viewportColumns?: number): boolean {
  const displayedRows = composerText.split("\n").map((row) => row.trimEnd());
  const textColumns = Number.isInteger(viewportColumns) ? viewportColumns! - COMPOSER_TEXT_COLUMN : null;
  let nextRow = 0;
  for (const expectedLine of expectedText.split("\n").map((line) => line.trimEnd())) {
    let shownLength = 0;
    do {
      const row = displayedRows[nextRow++];
      if (row === undefined || (row === "") !== (expectedLine === "") || !expectedLine.startsWith(row, shownLength)) return false;
      shownLength += row.length;
      if (shownLength === expectedLine.length) break;

      const spacesDroppedAtFold = textColumns === null ? null : countSpacesDroppedAtFold(expectedLine, shownLength, row, textColumns);
      if (spacesDroppedAtFold === null) return false;
      shownLength += spacesDroppedAtFold;
    } while (shownLength < expectedLine.length);
  }
  return nextRow === displayedRows.length;
}

/**
 * 프롬프트를 붙여넣은 뒤 Codex 입력란에 보여야 하는 글의 후보. `isSameComposerText`로 견줄 수 있게 화면 표기 그대로 만든다.
 *
 * Codex는 붙여넣은 이미지 경로를 순번이 붙은 자리표시로 바꾸고 뒤에 공백 하나를 두며, 긴 글은 글자 수만 적힌 자리표시로 접는다. 어느 길이부터 접는지는 Codex가 정하므로 두 형태를 모두 후보로 둔다.
 * 자리표시는 순번과 글자 수만 보여 주므로 붙여넣은 내용 자체를 증명하지는 못한다.
 */
export function expectCodexComposerTexts(body: string, imageCount: number): string[] {
  const imagePlaceholders = Array.from({ length: imageCount }, (_, index) => `[Image #${index + 1}] `).join("");
  return body ? [imagePlaceholders + body, `${imagePlaceholders}[Pasted Content ${[...body].length} chars]`] : [imagePlaceholders];
}
