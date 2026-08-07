import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useFlipReflow } from "../useFlipReflow";

const CARD_HEIGHT = 50;
const CONTAINER_TOP_AT_REST = 0;

/** 보드가 세로로 스크롤한 거리. 뷰포트 기준 좌표는 이 값만큼 통째로 밀린다 */
let scrollOffset = 0;
/** 지금 화면에 늘어놓인 카드 순서. 카드의 세로 위치를 여기서 계산한다 */
let currentOrder: string[] = [];

const originalGetBoundingClientRect = Element.prototype.getBoundingClientRect;
const originalAnimate = Element.prototype.animate;

function rectWithTop(top: number): DOMRect {
  return { top } as DOMRect;
}

function containerTop(): number {
  return CONTAINER_TOP_AT_REST - scrollOffset;
}

function Column({ ids }: { ids: string[] }) {
  const columnRef = useFlipReflow<HTMLDivElement>(ids.join(","));

  return (
    <div ref={columnRef} data-testid="column">
      {ids.map((id) => (
        <div key={id} data-kanban-task-id={id} />
      ))}
    </div>
  );
}

interface ShiftKeyframe {
  transform: string;
}

/** 세 번째 인자로 카드 id를 흘려 어떤 카드가 얼마나 미끄러졌는지 확인한다 */
function createAnimateSpy() {
  return vi.fn((_keyframes: ShiftKeyframe[], _options: unknown, _taskId: string | undefined) => {});
}

function readShift(animate: ReturnType<typeof createAnimateSpy>, taskId: string): number | null {
  const call = animate.mock.calls.find(([, , element]) => element === taskId);
  if (!call) return null;

  const [keyframes] = call;
  return Number(keyframes[0].transform.replace("translateY(", "").replace("px)", ""));
}

describe("useFlipReflow", () => {
  let animate: ReturnType<typeof createAnimateSpy>;

  beforeEach(() => {
    scrollOffset = 0;
    currentOrder = [];

    Element.prototype.getBoundingClientRect = function getBoundingClientRect(this: HTMLElement) {
      const taskId = this.dataset.kanbanTaskId;
      if (taskId) {
        return rectWithTop(containerTop() + currentOrder.indexOf(taskId) * CARD_HEIGHT);
      }
      if (this.dataset.testid === "column") {
        return rectWithTop(containerTop());
      }

      return rectWithTop(0);
    };

    animate = createAnimateSpy();
    Element.prototype.animate = function stubbedAnimate(
      this: HTMLElement,
      keyframes: unknown,
      options: unknown,
    ) {
      animate(keyframes as ShiftKeyframe[], options, this.dataset.kanbanTaskId);
      return {} as Animation;
    } as Element["animate"];
  });

  afterEach(() => {
    Element.prototype.getBoundingClientRect = originalGetBoundingClientRect;
    Element.prototype.animate = originalAnimate;
  });

  it("스크롤한 뒤 순서가 바뀌어도 스크롤한 거리가 아니라 자리 변화만큼만 미끄러진다", () => {
    // Given
    currentOrder = ["task-a", "task-b"];
    const { rerender } = render(<Column ids={currentOrder} />);

    // When
    /** 보드를 200px 내린 뒤 정렬 기준을 켜서 두 카드의 자리가 뒤바뀐 상황 */
    scrollOffset = 200;
    currentOrder = ["task-b", "task-a"];
    rerender(<Column ids={currentOrder} />);

    // Then
    /** 뷰포트 기준 top을 기억하면 스크롤한 200px이 그대로 섞여 엉뚱한 지점에서 날아온다 */
    expect(readShift(animate, "task-a")).toBe(-CARD_HEIGHT);
    expect(readShift(animate, "task-b")).toBe(CARD_HEIGHT);
  });

  it("자리가 그대로인 카드는 전환을 걸지 않는다", () => {
    // Given
    currentOrder = ["task-a", "task-b"];
    const { rerender } = render(<Column ids={currentOrder} />);

    // When
    /** 순서 자체가 바뀌어야 effect가 도므로 카드를 하나 더 붙이고 앞 두 장은 자리를 지킨다 */
    scrollOffset = 120;
    currentOrder = ["task-a", "task-b", "task-c"];
    rerender(<Column ids={currentOrder} />);

    // Then
    expect(readShift(animate, "task-a")).toBeNull();
    expect(readShift(animate, "task-b")).toBeNull();
  });
});
