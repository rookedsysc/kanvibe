import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AiSessionMessageContent } from "../AiSessionMessageContent";

vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));

const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
afterEach(() => {
  if (clipboardDescriptor) Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
  else Reflect.deleteProperty(navigator, "clipboard");
});

describe("AiSessionMessageContent", () => {
  it("언어가 붙은 코드 블록에 언어 이름을 보이고 글자에 색을 입힌다", () => {
    const { container } = render(<AiSessionMessageContent text={"```ts\nconst total = 3;\n```"} />);

    const header = container.querySelector(".kv-chat-code-header");
    expect(header?.querySelector("span")?.textContent).toBe("ts");
    expect(container.querySelector(".kv-chat-code pre code .hljs-keyword")?.textContent).toBe("const");
    expect(container.querySelector(".kv-chat-code pre")?.textContent).toBe("const total = 3;\n");
  });

  it("언어가 없거나 모르는 언어인 코드 블록은 색 없이 원문 그대로 둔다", () => {
    const { container } = render(<AiSessionMessageContent text={"```\n<b>plain</b>\n```\n\n```nosuchlang\nvalue = 1\n```"} />);

    const blocks = [...container.querySelectorAll(".kv-chat-code")];
    expect(blocks.map((block) => block.querySelector(".kv-chat-code-header span")?.textContent)).toEqual(["text", "nosuchlang"]);
    expect(blocks.map((block) => block.querySelector("pre")?.textContent)).toEqual(["<b>plain</b>\n", "value = 1\n"]);
    expect(container.querySelector("[class^='hljs-']")).toBeNull();
  });

  it("복사 버튼은 머리줄을 뺀 그 블록의 코드만 복사하고 결과를 알린다", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(<AiSessionMessageContent text={"```sh\necho first\n```\n\n```sh\necho second\n```"} />);

    fireEvent.click(screen.getAllByRole("button", { name: "aiSessions.copyMessage" })[1]);

    await waitFor(() => expect(screen.getByRole("button", { name: "aiSessions.copiedMessage" })).toBeTruthy());
    expect(writeText).toHaveBeenCalledWith("echo second\n");
  });

  it("복사에 실패하면 실패를 알리고 코드는 그대로 남긴다", async () => {
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) } });
    const { container } = render(<AiSessionMessageContent text={"```sh\necho kept\n```"} />);

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.copyMessage" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "aiSessions.copyFailed" })).toBeTruthy());
    expect(container.querySelector("pre")?.textContent).toBe("echo kept\n");
  });

  it("닫히지 않은 코드 블록도 끝까지 코드로 그린다", () => {
    const { container } = render(<AiSessionMessageContent text={"설명\n\n```py\nprint('still code')"} />);

    expect(container.querySelector(".kv-chat-code-header span")?.textContent).toBe("py");
    expect(container.querySelector(".kv-chat-code pre")?.textContent).toContain("print('still code')");
  });

  it("표를 옆으로 넘길 수 있는 틀로 감싼다", () => {
    const { container } = render(<AiSessionMessageContent text={"| 이름 | 값 |\n| --- | --- |\n| a | 1 |"} />);

    const table = container.querySelector(".kv-chat-table > table");
    expect(table?.querySelectorAll("td")).toHaveLength(2);
  });

  it("본문에 섞인 스크립트와 인라인 스타일은 그리지 않는다", () => {
    const { container } = render(<AiSessionMessageContent text={"<script>window.hacked = true</script><p style=\"color:red\">안전한 문단</p>"} />);

    expect(container.querySelector("script")).toBeNull();
    expect(screen.getByText("안전한 문단").getAttribute("style")).toBeNull();
  });
});
