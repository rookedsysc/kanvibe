import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PaneChatPromptCard } from "../PaneChatPromptCard";
import type { PaneChatPrompt } from "@/desktop/shared/paneChat";

vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
afterEach(cleanup);

const option = (number: number, label: string, extra: Partial<PaneChatPrompt["options"][number]> = {}) => ({ number, label, description: "", isChecked: false, isTextEntry: false, ...extra });
const question: PaneChatPrompt = {
  id: "prompt-question", kind: "question", title: "Which color do you prefer?", detail: "", stepsLine: null,
  highlightedNumber: 1, isMultiSelect: false, isTextEntryActive: false, hasHiddenOptions: false,
  options: [option(1, "Red", { description: "Warm" }), option(2, "Green", { description: "Calm" }), option(3, "Type something.", { isTextEntry: true })],
};

function open(prompt: PaneChatPrompt, props: { isBusy?: boolean; error?: string | null } = {}) {
  const onAnswer = vi.fn();
  const onTerminal = vi.fn();
  render(<PaneChatPromptCard prompt={prompt} isBusy={props.isBusy ?? false} error={props.error ?? null} onAnswer={onAnswer} onTerminal={onTerminal} />);
  return { onAnswer, onTerminal };
}

describe("PaneChatPromptCard", () => {
  it("AI의 질문을 제목과 설명이 붙은 선택지로 보여 주고, 고른 선택지의 번호를 답으로 보낸다", () => {
    const { onAnswer } = open(question);

    expect(screen.getByText("promptQuestion")).toBeTruthy();
    expect(screen.getByText("Which color do you prefer?")).toBeTruthy();
    expect(screen.getByText("Calm")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Green/ }));

    expect(onAnswer).toHaveBeenCalledWith({ type: "option", number: 2 });
  });

  it("직접 입력 줄은 입력란으로 보여 주고, 글을 적어 제출하면 그 글을 답으로 보낸다", () => {
    const { onAnswer } = open(question);
    const submit = screen.getByRole("button", { name: "promptSubmitText" });

    expect(submit).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByRole("textbox", { name: "promptTextPlaceholder" }), { target: { value: "Purple" } });
    fireEvent.click(submit);

    expect(onAnswer).toHaveBeenCalledWith({ type: "text", number: 3, text: "Purple" });
  });

  it("글이 이미 들어 있는 단일 선택의 직접 입력 줄은 입력란 대신 그 글을 보여 주고, 그 글로 답하게 한다", () => {
    const { onAnswer } = open({ ...question, isTextEntryActive: true, highlightedNumber: 3, options: [question.options[0], question.options[1], option(3, "Purple", { isTextEntry: true, isChecked: true })] });

    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.getByText("Purple")).toBeTruthy();
    expect(screen.getByText("promptTextEntered")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "promptSubmitText" }));

    expect(onAnswer).toHaveBeenCalledWith({ type: "text", number: 3, text: "Purple" });
  });

  it("다중 선택에서 글이 들어 있는 직접 입력 줄은 이미 고른 답이라 답하기 버튼을 두지 않는다", () => {
    open({ ...question, isMultiSelect: true, isTextEntryActive: true, highlightedNumber: 3, options: [option(1, "Cheese"), option(2, "Olives"), option(3, "Garlic", { isTextEntry: true, isChecked: true })] });

    expect(screen.getByText("Garlic")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "promptSubmitText" })).toBeNull();
  });

  it("글을 입력받는 중이어도 질문 이동 버튼을 누를 수 있다", () => {
    open({ ...question, stepsLine: "←  ☐ Size  ✔ Submit  →", isTextEntryActive: true, highlightedNumber: 3 });

    expect(screen.getByRole("button", { name: "promptNext" })).toHaveProperty("disabled", false);
  });

  it("화면에 다 보이지 않는 선택지가 있으면 나머지는 터미널에서 확인하라고, 선택지 목록보다 먼저 알린다", () => {
    open({ ...question, hasHiddenOptions: true });
    const notice = screen.getByText("promptHiddenOptions");
    expect(notice.compareDocumentPosition(screen.getByRole("list")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    cleanup();
    open(question);
    expect(screen.queryByText("promptHiddenOptions")).toBeNull();
  });

  it("다중 선택은 체크 상태를 누름 상태로 알리고, 단계가 있으면 이전·다음 질문으로 옮기는 답을 보낸다", () => {
    const { onAnswer } = open({ ...question, isMultiSelect: true, stepsLine: "←  ☒ Size  ☐ Toppings  ✔ Submit  →", options: [option(1, "Cheese", { isChecked: true }), option(2, "Olives")] });

    expect(screen.getByText("← ☒ Size ☐ Toppings ✔ Submit →")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Cheese/, pressed: true })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Olives/, pressed: false })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "promptNext" }));
    fireEvent.click(screen.getByRole("button", { name: "promptPrevious" }));

    expect(onAnswer.mock.calls).toEqual([[{ type: "step", direction: "next" }], [{ type: "step", direction: "previous" }]]);
  });

  it("단계가 없는 화면에는 질문 이동 버튼을 두지 않는다", () => {
    open(question);

    expect(screen.queryByRole("button", { name: "promptNext" })).toBeNull();
  });

  it("권한 확인은 CLI가 보여 준 도구와 실행하려는 내용을 보여 주고, 취소와 터미널 전환을 할 수 있다", () => {
    const { onAnswer, onTerminal } = open({ ...question, kind: "permission", title: "Do you want to proceed?", detail: "Bash command\n\ntouch probe.txt", options: [option(1, "Yes"), option(2, "No")] });

    expect(screen.getByText("promptPermission")).toBeTruthy();
    expect(screen.getByText(/Bash command\s+touch probe\.txt/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "terminal" }));

    expect(onAnswer).toHaveBeenCalledWith({ type: "cancel" });
    expect(onTerminal).toHaveBeenCalledTimes(1);
  });

  it("CLI의 확인은 그 종류로 표시한다", () => {
    open({ ...question, kind: "confirmation", title: "Switch model?", options: [option(1, "Yes"), option(2, "No")] });

    expect(screen.getByText("promptConfirmation")).toBeTruthy();
  });

  it("답을 보내는 중에는 선택지를 누를 수 없고, 보내지 못한 이유를 알린다", () => {
    open(question, { isBusy: true, error: "CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다." });

    expect(screen.getByRole("button", { name: /Red/ })).toHaveProperty("disabled", true);
    expect(screen.getByRole("alert").textContent).toBe("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.");
  });

  it("숫자 키 하나로 고를 수 없는 두 자리 번호의 선택지는 누를 수 없다", () => {
    open({ ...question, options: [...Array.from({ length: 9 }, (_, index) => option(index + 1, `Choice ${index + 1}`)), option(10, "Tenth")] });

    expect(screen.getByRole("button", { name: /Tenth/ })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: /Choice 9/ })).toHaveProperty("disabled", false);
  });
});
