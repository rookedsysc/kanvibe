import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import ProjectIcon from "@/components/ProjectIcon";

const ICON_DATA_URL = "data:image/png;base64,iVBORw0KGgo=";

describe("ProjectIcon", () => {
  it("GitHub 아이콘이 있으면 프로젝트명 앞 아이콘으로 그린다", () => {
    render(<ProjectIcon projectName="kanvibe" iconDataUrl={ICON_DATA_URL} />);

    const icon = screen.getByAltText("kanvibe icon");
    expect(icon.getAttribute("src")).toBe(ICON_DATA_URL);
  });

  it("아이콘이 없는 프로젝트는 아무것도 그리지 않는다", () => {
    const { container } = render(<ProjectIcon projectName="kanvibe" iconDataUrl={null} />);

    expect(container.innerHTML).toBe("");
  });
});
