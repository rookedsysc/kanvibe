import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useFlipReflow } from "../useFlipReflow";

const CARD_HEIGHT = 50;
const CONTAINER_TOP_AT_REST = 0;

/** 보드가 세로로 스크롤한 거리. 뷰포트 기준 좌표는 이 값만큼 통째로 밀린다 */
let scrollOffset = 0;
/** 지금 화면에 늘어놓인 카드 순서. 카드의 세로 위치를 여기서 계산한다 */
let currentOrder: string[] = [];
/** 기본 높이와 다른 카드만 담는다. PR 배지가 생기는 등으로 카드가 커지는 상황을 흉내낸다 */
let cardHeights: Record<string, number> = {};
/** 살아 있는 ResizeObserver 콜백. jsdom에는 구현이 없어 테스트가 직접 흘려준다 */
let resizeCallbacks: ResizeObserverCallback[] = [];

const originalGetBoundingClientRect = Element.prototype.getBoundingClientRect;
const originalAnimate = Element.prototype.animate;

function rectWithTop(top: number): DOMRect {
  return { top } as DOMRect;
}

function containerTop(): number {
  return CONTAINER_TOP_AT_REST - scrollOffset;
}

function cardTopWithinColumn(taskId: string): number {
  const index = currentOrder.indexOf(taskId);

  return currentOrder
    .slice(0, index)
    .reduce((top, id) => top + (cardHeights[id] ?? CARD_HEIGHT), 0);
}

class StubResizeObserver implements ResizeObserver {
  constructor(private readonly callback: ResizeObserverCallback) {
    resizeCallbacks.push(callback);
  }

  observe(): void {}
  unobserve(): void {}

  disconnect(): void {
    resizeCallbacks = resizeCallbacks.filter((candidate) => candidate !== this.callback);
  }
}

function triggerResize(): void {
  for (const callback of [...resizeCallbacks]) {
    callback([], {} as ResizeObserver);
  }
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
    cardHeights = {};
    resizeCallbacks = [];
    vi.stubGlobal("ResizeObserver", StubResizeObserver);

    Element.prototype.getBoundingClientRect = function getBoundingClientRect(this: HTMLElement) {
      const taskId = this.dataset.kanbanTaskId;
      if (taskId) {
        return rectWithTop(containerTop() + cardTopWithinColumn(taskId));
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
    vi.unstubAllGlobals();
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

  it("순서가 그대로인 채 카드 높이만 바뀌어도 다음 재정렬은 새 자리에서 출발한다", () => {
    // Given
    currentOrder = ["task-a", "task-b"];
    const { rerender } = render(<Column ids={currentOrder} />);

    /** 순서는 그대로인데 첫 카드에 PR 배지가 붙어 40px 커졌다 */
    cardHeights = { "task-a": CARD_HEIGHT + 40 };
    rerender(<Column ids={currentOrder} />);
    triggerResize();

    // When
    currentOrder = ["task-b", "task-a"];
    rerender(<Column ids={currentOrder} />);

    // Then
    /** 높이가 바뀌기 전 자리(50)를 기억하고 있으면 카드가 40px 어긋난 지점에서 날아온다 */
    expect(readShift(animate, "task-b")).toBe(CARD_HEIGHT + 40);
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
