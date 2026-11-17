import { PERMISSION_TITLE, type PaneChatPromptOption, type PaneChatPromptScreen } from "./paneChatClaudePrompt";

/** 선택지 한 줄. 강조된 줄은 첫 칸에 ›가 붙는다 */
const OPTION_LINE = /^(›| ) (\d+)\. (.+)$/;
/** 선택 화면 맨 아래의 조작 안내. 예: "enter select · esc back", "Press enter to confirm or esc to cancel" */
const KEY_HINT_LINE = /^ {2}\S.*\besc\b/;
/** 선택 화면의 제목과 상세는 두 칸 들여쓴다. 대화 내용은 첫 칸의 기호로 시작하거나 더 깊이 들여쓴다 */
const DIALOG_TEXT_LINE = /^ {2}\S/;
/** 선택지와 같은 줄에서 설명을 떼어 놓는 간격 */
const INLINE_DESCRIPTION_GAP = /\s{2,}/;
/** pane이 좁아 넘어간 선택지의 나머지 줄. 라벨이나 설명이 시작한 칸에 맞춰 들여쓴다 */
const WRAPPED_OPTION_ROW = /^ {5,}\S/;

const isBlank = (line: string | undefined) => !line?.trim();

/**
 * Codex CLI가 띄운 번호 선택 화면을 읽는다. 명령 승인, 사용량 경고의 모델 전환, 모델 목록이 같은 모양이다.
 *
 * Codex는 선택 화면을 화면 아래에 붙여 그리고 커서를 선택지 위에 두지 않는다. 그래서 맨 아래의 조작 안내 줄,
 * 그 위의 빈 줄, 그 위의 번호 목록이라는 배치로 판별한다. 선택 화면 위쪽은 빈 줄 둘로 대화 내용과 떨어져 있다.
 * Codex CLI 0.169.0의 실제 화면으로 확인한 배치이고, 읽지 못하는 화면은 null이다.
 */
export function readCodexPrompt(lines: string[]): PaneChatPromptScreen | null {
  let hintRow = lines.length - 1;
  while (hintRow >= 0 && isBlank(lines[hintRow])) hintRow--;
  if (hintRow < 0 || !KEY_HINT_LINE.test(lines[hintRow]) || !isBlank(lines[hintRow - 1])) return null;

  const lastOptionRow = hintRow - 2;
  let firstOptionRow = lastOptionRow + 1;
  while (firstOptionRow > 0 && (OPTION_LINE.test(lines[firstOptionRow - 1]) || WRAPPED_OPTION_ROW.test(lines[firstOptionRow - 1]))) firstOptionRow--;
  const options = readOptions(lines.slice(firstOptionRow, lastOptionRow + 1));
  const highlighted = options.filter((option) => option.isHighlighted);
  if (options.length < 2 || highlighted.length !== 1 || options.some((option, index) => option.number !== index + 1)) return null;

  let textEnd = firstOptionRow - 1;
  while (textEnd >= 0 && isBlank(lines[textEnd])) textEnd--;
  let textStart = textEnd + 1;
  // 빈 줄 하나는 선택 화면 안의 문단 사이, 빈 줄 둘은 선택 화면의 위쪽 끝이다
  while (textStart > 0 && (DIALOG_TEXT_LINE.test(lines[textStart - 1]) || (isBlank(lines[textStart - 1]) && DIALOG_TEXT_LINE.test(lines[textStart - 2] ?? "")))) textStart--;
  const paragraphs = lines.slice(textStart, textEnd + 1).map((line) => line.trim()).join("\n").split(/\n{2,}/).filter(Boolean);
  if (!paragraphs.length) return null;

  return {
    kind: PERMISSION_TITLE.test(paragraphs[0]) ? "permission" : "confirmation",
    title: paragraphs[0],
    detail: paragraphs.slice(1).join("\n\n"),
    stepsLine: null,
    options: options.map(({ isHighlighted: _isHighlighted, ...option }) => option),
    highlightedNumber: highlighted[0].number,
    isMultiSelect: false,
    isTextEntryActive: false,
    hasHiddenOptions: false,
  };
}

/**
 * 선택지 줄과, pane이 좁아 그 아래로 넘어간 줄을 묶는다.
 * 넘어간 줄이 설명 칸에서 시작하면 설명의 나머지이고, 그보다 왼쪽이면 라벨의 나머지다. Codex는 붙임표 뒤에서도 줄을 나누므로 그때는 공백 없이 잇는다.
 */
function readOptions(optionLines: string[]): (PaneChatPromptOption & { isHighlighted: boolean })[] {
  const options: (PaneChatPromptOption & { isHighlighted: boolean })[] = [];
  let descriptionColumn = Infinity;
  for (const line of optionLines) {
    const optionLine = OPTION_LINE.exec(line);
    if (optionLine) {
      const [label, ...description] = optionLine[3].trim().split(INLINE_DESCRIPTION_GAP);
      descriptionColumn = description.length ? line.indexOf(description[0], line.indexOf(label) + label.length) : Infinity;
      options.push({ number: Number(optionLine[2]), label, description: description.join(" "), isChecked: false, isTextEntry: false, isHighlighted: optionLine[1] === "›" });
      continue;
    }
    const current = options.at(-1);
    if (!current) continue;
    const rowText = line.trim();
    if (line.length - line.trimStart().length >= descriptionColumn) current.description = `${current.description} ${rowText}`;
    else current.label = `${current.label}${current.label.endsWith("-") ? "" : " "}${rowText}`;
  }
  return options;
}
