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

  it("아이콘이 없으면 프로젝트 색상의 이니셜 배지를 그린다", () => {
    render(<ProjectIcon projectName="internal-tool" iconDataUrl={null} color="#86EFAC" />);

    const badge = screen.getByTestId("project-initial-icon");
    expect(badge.textContent).toBe("i");
    expect(badge.style.backgroundColor).toBe("rgb(134, 239, 172)");
    /** 파스텔 배경 위에서는 어두운 글자로 대비를 확보한다 */
    expect(badge.style.color).toBe("rgb(17, 24, 39)");
  });

  it("어두운 프로젝트 색상에서는 이니셜을 흰색으로 그린다", () => {
    render(<ProjectIcon projectName="kanvibe" iconDataUrl={null} color="#0064FF" />);

    expect(screen.getByTestId("project-initial-icon").style.color).toBe("rgb(255, 255, 255)");
  });

  it("아이콘이 있으면 색상이 있어도 이니셜 배지를 그리지 않는다", () => {
    render(<ProjectIcon projectName="kanvibe" iconDataUrl={ICON_DATA_URL} color="#86EFAC" />);

    expect(screen.queryByTestId("project-initial-icon")).toBeNull();
  });

  it("색상이 없으면 아무것도 그리지 않는다", () => {
    const { container } = render(<ProjectIcon projectName="kanvibe" iconDataUrl={null} />);

    expect(container.innerHTML).toBe("");
  });
});
