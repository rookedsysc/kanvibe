/** @vitest-environment node */
import { describe, expect, it } from "vitest";
import currentSession from "./fixtures/herdr/current-session-v3.json";
import currentDevSession from "./fixtures/herdr/current-dev-session-v3.json";
import { decodeHerdrSnapshot } from "@/lib/herdrSnapshot";

// v0.9.5 공식 tests/fixtures/session/current-herdr(-dev)-session.json을 사용한다.
// 포맷 정본: https://github.com/herdrdev/herdr/blob/v0.9.5/src/persist/snapshot.rs
function snapshot() {
  return structuredClone(currentSession);
}

function splitSnapshot() {
  const value = structuredClone(currentDevSession);
  const layout = value.workspaces[1].tabs[0].layout as {
    Split: { direction: string; ratio: number; first: { Pane: number }; second: { Pane: number } };
  };
  return { value, split: layout.Split };
}

function decode(value: unknown) {
  return decodeHerdrSnapshot(JSON.stringify(value));
}

describe("Herdr snapshot decoder", () => {
  it("공식 version 3 fixture의 작업 경로, 탭, 원본 ID를 보존한다", () => {
    const result = decode(currentSession);

    expect(result.version).toBe(3);
    expect(result.workspaces).toHaveLength(2);
    expect(result.workspaces[0]).toEqual({
      sourceWorkspaceId: "wproj1",
      label: "wproj1",
      cwd: "/home/test/projects/project-a",
      activeTabIndex: 0,
      tabs: [
        {
          label: "separate-pane",
          layout: { type: "pane", sourcePaneId: "1" },
          panes: [{ sourcePaneId: "1", cwd: "/home/test/projects/project-a", label: null, agentSession: null }],
          focusedPaneId: "1", rootPaneId: "1", zoomed: false,
        },
        {
          label: "p",
          layout: { type: "pane", sourcePaneId: "2" },
          panes: [{ sourcePaneId: "2", cwd: "/home/test/projects/project-a", label: null, agentSession: null }],
          focusedPaneId: "2", rootPaneId: "2", zoomed: false,
        },
      ],
    });
  });

  it("공식 additive fixture의 활성 탭과 수직 분할을 읽는다", () => {
    const result = decode(currentDevSession);

    expect(result.workspaces[0].activeTabIndex).toBe(1);
    expect(result.workspaces[1].tabs[0]).toMatchObject({
      layout: { type: "split", direction: "down", ratio: 0.5,
        first: { type: "pane", sourcePaneId: "6" },
        second: { type: "pane", sourcePaneId: "7" } },
      focusedPaneId: "6", rootPaneId: "6",
    });
  });

  it("원본 사용자 이름과 대화 참조만 보존하고 실행 명령은 반환하지 않는다", () => {
    const value = snapshot();
    const pane = (value.workspaces[0].tabs[0].panes as Record<string, { cwd: string }>)["1"];
    Object.assign(value.workspaces[0], { custom_name: "도트파일" });
    Object.assign(pane, {
      label: "코드 작업",
      agent_session: { source: "codex", agent: "codex", kind: "id", value: "conversation-id" },
      agent_resume: { source: "custom", agent: "custom", argv: ["sh", "-c", "touch /tmp/unsafe"] },
      launch_argv: ["sh", "-c", "touch /tmp/unsafe"],
    });

    const result = decode(value);

    expect(result.workspaces[0].label).toBe("도트파일");
    expect(result.workspaces[0].tabs[0].panes[0]).toEqual({
      sourcePaneId: "1", cwd: "/home/test/projects/project-a", label: "코드 작업",
      agentSession: { source: "codex", agent: "codex", kind: "id", value: "conversation-id" },
    });
    expect(JSON.stringify(result)).not.toContain("touch /tmp/unsafe");
  });

  it("선택 필드가 없으면 첫 pane과 첫 탭을 사용한다", () => {
    const value = snapshot();
    const first = value.workspaces[0];
    delete (first as Partial<typeof first>).active_tab;
    delete (first.tabs[0] as Partial<typeof first.tabs[0]>).focused;
    delete (first.tabs[0] as Partial<typeof first.tabs[0]>).root_pane;

    expect(decode(value).workspaces[0]).toMatchObject({
      activeTabIndex: 0,
      tabs: [{ focusedPaneId: "1", rootPaneId: "1" }, { focusedPaneId: "2", rootPaneId: "2" }],
    });
  });

  it.each([0, 1, 2, 4, 999, -1, "3", null, undefined])("미지원 version %s는 거부한다", (version) => {
    expect(() => decode({ ...currentSession, version })).toThrow(/version|형식/);
  });

  it.each(["not-json", "[]", "null", '{"version":3}', '{"version":3,"workspaces":{}}'])
    ("잘못된 최상위 자료를 거부한다: %s", (content) => {
      expect(() => decodeHerdrSnapshot(content)).toThrow();
    });

  it.each(["relative/path", "", "/tmp/line\nbreak", "/tmp/null\u0000path"])("안전하지 않은 작업 경로를 거부한다: %s", (cwd) => {
    const value = snapshot();
    value.workspaces[0].identity_cwd = cwd;
    expect(() => decode(value)).toThrow(/경로/);
  });

  it("호스트에서 해석할 홈 경로를 보존한다", () => {
    const value = snapshot();
    value.workspaces[0].identity_cwd = "~/projects/dotfiles";
    (value.workspaces[0].tabs[0].panes as Record<string, { cwd: string }>)["1"].cwd = "~/projects/dotfiles";
    expect(decode(value).workspaces[0].cwd).toBe("~/projects/dotfiles");
  });

  it.each(["w1:p1", "", "../../outside"])("workspace ID를 경로나 pane ID로 오해하지 않는다: %s", (id) => {
    const value = snapshot();
    value.workspaces[0].id = id;
    expect(() => decode(value)).toThrow(/ID/);
  });

  it("중복 workspace ID를 거부한다", () => {
    const value = snapshot();
    value.workspaces[1].id = value.workspaces[0].id;
    expect(() => decode(value)).toThrow(/중복/);
  });

  it("배치가 참조하는 pane이 없으면 거부한다", () => {
    const value = snapshot();
    value.workspaces[0].tabs[0].layout.Pane = 999;
    expect(() => decode(value)).toThrow(/pane/);
  });

  it.each([0, 1, -0.1, 2])("잘못된 분할 비율 %s를 거부한다", (ratio) => {
    const { value, split } = splitSnapshot();
    split.ratio = ratio;
    expect(() => decode(value)).toThrow(/비율/);
  });

  it("알 수 없는 분할 방향은 기본 방향으로 바꾸지 않는다", () => {
    const { value, split } = splitSnapshot();
    split.direction = "Diagonal";
    expect(() => decode(value)).toThrow(/방향/);
  });

  it("같은 pane이 배치에 두 번 나타나면 거부한다", () => {
    const { value, split } = splitSnapshot();
    split.second.Pane = 6;
    expect(() => decode(value)).toThrow(/중복/);
  });

  it("활성 탭과 focused pane의 존재를 확인한다", () => {
    const value = snapshot();
    value.workspaces[0].active_tab = 10;
    expect(() => decode(value)).toThrow(/탭/);
    value.workspaces[0].active_tab = 0;
    value.workspaces[0].tabs[0].focused = 99;
    expect(() => decode(value)).toThrow(/pane/);
  });

  it("수평 분할은 오른쪽 분할로 변환한다", () => {
    const { value, split } = splitSnapshot();
    split.direction = "Horizontal";
    expect(decode(value).workspaces[1].tabs[0].layout).toMatchObject({ type: "split", direction: "right" });
  });

  it("각 pane의 작업 경로도 검증한다", () => {
    const value = snapshot();
    const pane = (value.workspaces[0].tabs[0].panes as Record<string, { cwd?: string }>)["1"];
    delete pane.cwd;
    expect(() => decode(value)).toThrow(/경로/);
  });

  it("workspace ID가 없는 version 3 자료는 임의 ID를 만들지 않는다", () => {
    const value = snapshot();
    delete (value.workspaces[0] as Partial<typeof value.workspaces[0]>).id;
    expect(() => decode(value)).toThrow(/ID/);
  });

  it("배치에서 사용하지 않는 pane 자료는 조용히 버리지 않는다", () => {
    const value = snapshot();
    Object.assign(value.workspaces[0].tabs[0].panes, { "99": { cwd: "/tmp" } });
    expect(() => decode(value)).toThrow(/pane/);
  });

  it("알 수 없는 대화 참조 형식을 실행 가능한 명령으로 해석하지 않는다", () => {
    const value = snapshot();
    const pane = (value.workspaces[0].tabs[0].panes as Record<string, { cwd: string }>)["1"];
    Object.assign(pane, { agent_session: { source: "custom", agent: "custom", kind: "command", value: "sh -c unsafe" } });
    expect(() => decode(value)).toThrow(/참조 형식/);
  });

  it("지나치게 깊은 배치는 재귀 처리 전에 제한한다", () => {
    const value = snapshot();
    const panes: Record<string, { cwd: string }> = {};
    let layout: unknown = { Pane: 0 };
    panes["0"] = { cwd: "/tmp" };
    for (let id = 1; id <= 66; id++) {
      panes[String(id)] = { cwd: "/tmp" };
      layout = { Split: { direction: "Horizontal", ratio: 0.5, first: layout, second: { Pane: id } } };
    }
    Object.assign(value.workspaces[0].tabs[0], { panes, layout, root_pane: 0, focused: 0 });
    expect(() => decode(value)).toThrow(/깊/);
  });

  it("빈 snapshot은 작업이 없는 정상 결과다", () => {
    expect(decode({ version: 3, workspaces: [], active: null, selected: 0 })).toEqual({ version: 3, workspaces: [] });
  });
});
