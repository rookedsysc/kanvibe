export interface PaneChatPromptOption {
  /** CLI 화면에 적힌 번호. 이 숫자 키를 누르면 CLI가 그 항목을 고른다 */
  number: number;
  label: string;
  /** 선택지 아래에 딸린 글. 화면 너비를 넘겨 다음 줄로 넘어간 선택지의 나머지도 여기에 온다 */
  description: string;
  /** 다중 선택에서 체크했거나, 앞서 이 질문에 고른 답인 항목 */
  isChecked: boolean;
  /** 고르면 그 줄에 글을 직접 입력받는 항목 */
  isTextEntry: boolean;
}

/** CLI가 답을 기다리며 띄운 선택 화면을 읽은 결과 */
export interface PaneChatPromptScreen {
  /** question은 AI가 작업 중 던진 질문, permission은 도구 실행 허용 요청, confirmation은 CLI가 스스로 묻는 확인이다 */
  kind: "question" | "permission" | "confirmation";
  title: string;
  /** 제목 위에 CLI가 함께 보여 주는 내용. 권한 확인의 명령, 답 확인 화면의 고른 답 등이다 */
  detail: string;
  /** 질문이 여러 개일 때 맨 위에 나오는 단계 줄. 좌우 화살표 키로 단계를 옮길 수 있다는 표시다 */
  stepsLine: string | null;
  options: PaneChatPromptOption[];
  /** CLI가 지금 강조하고 있는 선택지의 번호 */
  highlightedNumber: number;
  isMultiSelect: boolean;
  /** 강조된 줄이 글을 입력받는 중인지. 이때 숫자 키는 선택이 아니라 글자로 들어간다 */
  isTextEntryActive: boolean;
  /** pane이 짧아 CLI가 선택지를 스크롤해 보여 주는 중이라, 화면에 없는 선택지가 더 있는지 */
  hasHiddenOptions: boolean;
}

/** 선택지 한 줄. 강조된 줄은 번호 앞에 ❯가 붙고 나머지 줄은 그 자리가 비어 있다. 스크롤된 목록은 가려진 쪽 끝 줄의 그 자리에 화살표를 둔다 */
const OPTION_LINE = /^(\s*)(❯|↑|↓|\s) (\d+)\. (.*)$/;
/** 선택 화면이 시작되는 위쪽 경계선. 그 위는 대화 내용이다. CLI가 이 줄에 안내 글을 넣으면 경계선은 줄 끝에만 남는다 */
const TOP_BORDER = /^\s*[─▔]{10,}\s*$|\s[─▔]+\s*$/;
/** 선택 화면 안에서 내용을 나누는 가로줄 */
const DIVIDER = /^\s*[─╌▔]{3,}\s*$/;
const CHECKBOX = /^\[(.)\] (.*)$/;
/** 앞서 고른 답 뒤에 붙는 표시. 글을 입력받는 줄에서는 글과 표시 사이에 커서 자리가 한 칸 더 낀다 */
const CHOSEN_MARK = /\s+✔(?=\s|$)/;
/** AI의 질문 화면에만 있는 선택지 */
const QUESTION_ONLY_OPTION = "Chat about this";
/** 도구 실행이나 계획 진행의 허용을 묻는 화면의 제목. Claude와 Codex가 같은 말로 묻는다 */
export const PERMISSION_TITLE = /^(?:Do you want|Would you like) to /;
const STEPS_LINE = /^←\s.*\s→$/;
/** 글을 직접 입력받는 줄이 비어 있을 때 Claude가 보여 주는 자리 표시 글 */
const TEXT_ENTRY_PLACEHOLDER = /^Type something\.?$/;
/** 번호 칸에서 선택지 글이나 설명이 시작하는 칸까지의 최소 들여쓰기 */
const OPTION_BODY_INDENT = 3;
/** 선택지와 제목 사이에 허용하는 빈 줄 수. 더 떨어져 있으면 그 글은 이 선택지의 제목이 아니다 */
const MAX_BLANK_ROWS_BELOW_TITLE = 1;

/** 직접 입력 줄에 아직 아무 글도 넣지 않았는지 */
export function isBlankClaudeTextEntry(option: PaneChatPromptOption): boolean {
  return option.isTextEntry && TEXT_ENTRY_PLACEHOLDER.test(option.label);
}

const indentOf = (line: string) => line.length - line.trimStart().length;

/**
 * Claude Code가 띄운 번호 선택 화면을 읽는다. AI의 질문, 도구 실행 허용 요청, 모델 전환 확인이 모두 같은 모양이다.
 *
 * Claude는 선택 화면이 떠 있는 동안 커서를 강조된 선택지의 ❯ 위에 두고, 그 줄에 글을 입력받는 동안에는 글 위에 둔다.
 * 입력란이나 대화 본문에 번호 목록이 있어도 속지 않도록 이 커서 위치와, 선택지 바로 위에 제목이 있는지를 함께 본다.
 * Claude Code 2.1.326의 실제 화면으로 확인한 배치이고, 읽지 못하는 화면은 null이다.
 * 화면 너비를 주면 너비를 넘겨 다음 줄로 넘어간 선택지를 한 라벨로 이어 붙인다. 너비를 모르면 넘어간 줄은 설명으로 남는다.
 */
export function readClaudePrompt(lines: string[], cursorRow: number, cursorColumn: number, viewportColumns?: number | null): PaneChatPromptScreen | null {
  const highlighted = OPTION_LINE.exec(lines[cursorRow] ?? "");
  if (!highlighted || highlighted[2] !== "❯") return null;
  const markerColumn = highlighted[1].length;
  const highlightedNumber = Number(highlighted[3]);
  const labelColumn = highlighted[0].length - highlighted[4].length;
  const isTextEntryActive = cursorColumn !== markerColumn;
  if (isTextEntryActive && cursorColumn < labelColumn) return null;

  const optionNumberAt = (row: number) => {
    const optionLine = OPTION_LINE.exec(lines[row] ?? "");
    return optionLine?.[1].length === markerColumn ? Number(optionLine[3]) : null;
  };
  const isInsideOptions = (row: number) => {
    const line = lines[row] ?? "";
    return optionNumberAt(row) !== null || DIVIDER.test(line) || (!!line.trim() && indentOf(line) >= markerColumn + OPTION_BODY_INDENT);
  };
  let firstRow = cursorRow;
  while (firstRow > 0 && isInsideOptions(firstRow - 1)) firstRow--;
  // 위로 더듬다 닿은 딸린 줄과 가로줄은 선택지가 아니다. 목록은 선택지 줄에서 시작한다
  while (optionNumberAt(firstRow) === null) firstRow++;
  let lastRow = cursorRow;
  while (lastRow + 1 < lines.length && isInsideOptions(lastRow + 1)) lastRow++;

  const optionLines = lines.slice(firstRow, lastRow + 1);
  const hasHiddenOptions = optionLines.some((line) => /[↑↓]/.test(OPTION_LINE.exec(line)?.[2] ?? ""));
  // Claude는 선택 화면의 글을 pane 너비보다 한 칸 좁게 접는다
  const wrapColumn = viewportColumns ? viewportColumns - 1 : null;
  const optionRows = readOptionRows(optionLines, markerColumn);
  const isNumberedInOrder = optionRows.every((option, index) => hasHiddenOptions ? index === 0 || option.number > optionRows[index - 1].number : option.number === index + 1);
  if (optionRows.length < 2 || !isNumberedInOrder) return null;

  let titleEnd = firstRow - 1;
  while (titleEnd >= 0 && !lines[titleEnd].trim() && firstRow - 1 - titleEnd < MAX_BLANK_ROWS_BELOW_TITLE) titleEnd--;
  let titleStart = titleEnd + 1;
  while (titleStart > 0 && lines[titleStart - 1].trim() && !DIVIDER.test(lines[titleStart - 1]) && !TOP_BORDER.test(lines[titleStart - 1])) titleStart--;
  if (titleStart > titleEnd) return null;

  let topBorderRow = titleStart - 1;
  while (topBorderRow >= 0 && !TOP_BORDER.test(lines[topBorderRow])) topBorderRow--;
  // 위쪽 경계선이 화면 밖으로 밀려나 있으면 어디부터가 선택 화면인지 알 수 없어 상세를 싣지 않는다
  const above = topBorderRow < 0 ? [] : lines.slice(topBorderRow + 1, titleStart);
  const stepsLine = above.map((line) => line.trim()).find((line) => STEPS_LINE.test(line)) ?? null;

  const titleLines = lines.slice(titleStart, titleEnd + 1).map((line) => line.trim());
  const isQuestion = !!stepsLine || optionRows.some((option) => option.label === QUESTION_ONLY_OPTION);
  const textEntryNumber = isTextEntryActive ? highlightedNumber : null;
  // 선택지 아래에 설명 줄이 따로 붙는 것은 AI의 질문뿐이다
  const options = optionRows.map((option) => describeOption(option, wrapColumn, isQuestion, textEntryNumber));

  return {
    kind: isQuestion ? "question" : PERMISSION_TITLE.test(titleLines.at(-1)!) ? "permission" : "confirmation",
    title: titleLines.join("\n"),
    detail: formatDetail(above.filter((line) => line.trim() !== stepsLine)),
    stepsLine,
    options,
    highlightedNumber,
    isMultiSelect: optionLines.some((line) => CHECKBOX.test(OPTION_LINE.exec(line)?.[4] ?? "")),
    isTextEntryActive,
    hasHiddenOptions,
  };
}

/** 선택지 한 개가 화면에서 차지한 줄들. 첫 줄의 글과, 그 아래 딸린 줄을 읽은 그대로 담는다 */
interface OptionRows {
  number: number;
  label: string;
  isChecked: boolean;
  /** 선택지 글이 시작하는 칸. 딸린 줄은 이 칸 이상으로 들여쓴다 */
  bodyColumn: number;
  firstRowLength: number;
  continuationRows: string[];
}

/** 선택지 줄과 그 아래 딸린 줄을 묶는다. 선택지 글보다 덜 들여쓴 줄(다중 선택의 Submit 줄)은 어느 선택지에도 속하지 않는다 */
function readOptionRows(optionLines: string[], markerColumn: number): OptionRows[] {
  const options: OptionRows[] = [];
  for (const line of optionLines) {
    const optionLine = OPTION_LINE.exec(line);
    if (optionLine && optionLine[1].length === markerColumn) {
      const checkbox = CHECKBOX.exec(optionLine[4]);
      const text = checkbox ? checkbox[2] : optionLine[4];
      options.push({
        number: Number(optionLine[3]),
        label: text.replace(CHOSEN_MARK, "").trim(),
        isChecked: checkbox ? checkbox[1] !== " " : CHOSEN_MARK.test(text),
        bodyColumn: line.length - text.length,
        firstRowLength: line.trimEnd().length,
        continuationRows: [],
      });
      continue;
    }
    const current = options.at(-1);
    if (current && !DIVIDER.test(line) && indentOf(line) >= current.bodyColumn) current.continuationRows.push(line);
  }
  return options;
}

/**
 * 딸린 줄을 라벨의 나머지와 설명으로 가른다.
 *
 * 너비를 넘겨 넘어온 줄은 앞줄에 그 첫 낱말이 들어갈 자리가 없었던 줄이다. 자리가 있었는데도 줄을 바꿨다면 거기서부터가 설명이다.
 * 모든 줄이 자리가 없었던 경우는 화면만으로 가를 수 없다. 설명 줄이 붙는 화면이면 첫 줄만 라벨로 보고, 아니면 전부 라벨로 본다.
 */
function describeOption(option: OptionRows, wrapColumn: number | null, hasDescriptionRows: boolean, textEntryNumber: number | null): PaneChatPromptOption {
  const rows = option.continuationRows.map((line) => ({ text: line.trim(), length: line.trimEnd().length }));
  let previousRowLength = option.firstRowLength;
  const couldBeWrapped = rows.map((row) => {
    const hadRoomForNextWord = wrapColumn === null || previousRowLength + 1 + row.text.split(" ")[0].length <= wrapColumn;
    previousRowLength = row.length;
    return !hadRoomForNextWord;
  });
  const firstSeparateRow = couldBeWrapped.indexOf(false);
  const labelRowCount = firstSeparateRow >= 0 ? firstSeparateRow : hasDescriptionRows ? 0 : rows.length;

  let label = option.label;
  previousRowLength = option.firstRowLength;
  for (const row of rows.slice(0, labelRowCount)) {
    // 한 줄에 다 들어가지 않는 낱말만 가운데에서 끊긴다. 그때는 끊긴 두 조각 사이에 공백이 없었다
    const brokenWord = `${label.split(" ").at(-1)}${row.text.split(" ")[0]}`;
    const isBrokenInsideWord = previousRowLength >= wrapColumn! && brokenWord.length > wrapColumn! - option.bodyColumn;
    label = `${label}${isBrokenInsideWord ? "" : " "}${row.text}`;
    previousRowLength = row.length;
  }
  return {
    number: option.number,
    label,
    description: rows.slice(labelRowCount).map((row) => row.text).join(" "),
    isChecked: option.isChecked,
    isTextEntry: TEXT_ENTRY_PLACEHOLDER.test(label) || option.number === textEntryNumber,
  };
}

/** 가로줄을 빈 줄로 바꾸고, 앞뒤 빈 줄과 공통 들여쓰기를 걷어 낸다 */
function formatDetail(detailLines: string[]): string {
  const lines = detailLines.map((line) => DIVIDER.test(line) ? "" : line.trimEnd());
  const indent = Math.min(...lines.filter(Boolean).map(indentOf));
  return lines.map((line) => line.slice(indent)).join("\n").replace(/\n{3,}/g, "\n\n").trim();
}
