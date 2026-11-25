//! 열 안 카드 순서가 바뀌면 카드가 이전 자리에서 새 자리로 미끄러져 오게 한다(TS `useFlipReflow`).
//! 정렬 기준을 켜거나 드롭한 카드가 정렬된 자리로 되밀릴 때 카드가 순간이동하지 않게 한다.
//!
//! 자리는 창이 아니라 열 위쪽 기준으로 기억한다. 보드는 세로로 스크롤되므로 창 기준으로 재면 스크롤한 뒤 정렬을
//! 바꿨을 때 스크롤한 거리만큼 어긋난 곳에서 카드가 날아온다. 자리는 그리는 프레임마다 다시 재므로 순서가 그대로인 채
//! 카드 높이만 바뀌어도(PR 배지가 생기는 등) 다음 재정렬은 새 자리에서 출발한다(TS의 `ResizeObserver` 몫).
//!
//! 레이아웃은 그대로 두고 그리기 단계에서만 카드를 옮긴다. 순서가 바뀐 그 프레임에 새 자리를 재고 바로 이전 자리에서
//! 그리므로 새 자리가 한 프레임 비치는 일이 없다. 이동 중에 다시 순서가 바뀌면 TS `composite: "add"`처럼 남은 이동에
//! 새 이동을 더한다. TS처럼 운영체제의 움직임 줄이기 설정과 상관없이 항상 움직인다(Web Animations API는
//! `prefers-reduced-motion`을 보지 않는다).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window, point, px,
};

/// TS `REFLOW_DURATION_MS`
const REFLOW_DURATION: Duration = Duration::from_millis(220);

/// 이보다 작게 움직인 카드는 전환을 걸지 않는다(TS `Math.abs(shift) < 1`)
const MIN_SHIFT: f32 = 1.0;

/// 카드 하나에 걸린 이동. `distance`(이전 자리 - 새 자리)에서 0으로 줄어든다
struct CardShift {
    distance: f32,
    started_at: Instant,
}

/// 열 하나의 카드 자리 기록
#[derive(Default)]
pub(super) struct ColumnReflow {
    order_key: String,
    column_top: f32,
    is_reordered: bool,
    /// 지난 프레임의 카드 자리(열 위쪽 기준)
    settled_offsets: HashMap<String, f32>,
    frame_offsets: HashMap<String, f32>,
    shifts: HashMap<String, Vec<CardShift>>,
}

pub(super) type SharedColumnReflow = Rc<RefCell<ColumnReflow>>;

impl ColumnReflow {
    fn begin_frame(&mut self, order_key: &str, column_top: f32) {
        self.is_reordered = self.order_key != order_key;
        self.order_key = order_key.to_string();
        self.column_top = column_top;
        self.frame_offsets.clear();
    }

    /// 카드의 새 자리를 적고, 지금 그릴 때 더할 세로 이동을 돌려준다
    fn place_card(&mut self, task_id: &str, card_top: f32, now: Instant) -> f32 {
        let offset = card_top - self.column_top;
        self.frame_offsets.insert(task_id.to_string(), offset);
        if self.is_reordered {
            let distance = self
                .settled_offsets
                .get(task_id)
                .map(|previous| previous - offset)
                .filter(|distance| distance.abs() >= MIN_SHIFT);
            if let Some(distance) = distance {
                self.shifts
                    .entry(task_id.to_string())
                    .or_default()
                    .push(CardShift {
                        distance,
                        started_at: now,
                    });
            }
        }
        let Some(shifts) = self.shifts.get_mut(task_id) else {
            return 0.0;
        };
        shifts.retain(|shift| now.saturating_duration_since(shift.started_at) < REFLOW_DURATION);
        shifts.iter().map(|shift| remaining_shift(shift, now)).sum()
    }

    /// 이번 프레임 자리를 다음 비교 기준으로 남긴다. 아직 움직이는 카드가 있으면 `true`
    fn end_frame(&mut self) -> bool {
        let frame_offsets = &self.frame_offsets;
        self.shifts
            .retain(|task_id, shifts| frame_offsets.contains_key(task_id) && !shifts.is_empty());
        self.settled_offsets = std::mem::take(&mut self.frame_offsets);
        self.is_reordered = false;
        !self.shifts.is_empty()
    }
}

fn remaining_shift(shift: &CardShift, now: Instant) -> f32 {
    let elapsed = now.saturating_duration_since(shift.started_at);
    let progress = (elapsed.as_secs_f32() / REFLOW_DURATION.as_secs_f32()).min(1.0);
    shift.distance * (1.0 - css_ease_out(progress))
}

/// CSS `ease-out`(`cubic-bezier(0, 0, 0.58, 1)`). 시간 축 x에서 진행 y를 이분법으로 찾는다
fn css_ease_out(progress: f32) -> f32 {
    let bezier = |t: f32, second: f32| 3.0 * (1.0 - t) * t * t * second + t * t * t;
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if bezier(middle, 0.58) < progress {
            low = middle;
        } else {
            high = middle;
        }
    }
    bezier((low + high) / 2.0, 1.0)
}

/// 열 전체(TS `columnRef`)를 감싸 그 위쪽을 자리 기준으로 삼는다. 레이아웃은 감싼 요소 그대로다
pub(super) struct FlipColumn {
    reflow: SharedColumnReflow,
    order_key: String,
    child: AnyElement,
}

pub(super) fn flip_column(
    reflow: &SharedColumnReflow,
    order_key: String,
    child: impl IntoElement,
) -> FlipColumn {
    FlipColumn {
        reflow: reflow.clone(),
        order_key,
        child: child.into_any_element(),
    }
}

/// 카드 하나(TS `[data-kanban-task-id]`)를 감싸 순서가 바뀐 만큼 그리기 위치를 옮긴다
pub(super) struct FlipCard {
    reflow: SharedColumnReflow,
    task_id: String,
    child: AnyElement,
}

pub(super) fn flip_card(reflow: &SharedColumnReflow, task_id: &str, child: AnyElement) -> FlipCard {
    FlipCard {
        reflow: reflow.clone(),
        task_id: task_id.to_string(),
        child,
    }
}

impl Element for FlipColumn {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.reflow
            .borrow_mut()
            .begin_frame(&self.order_key, f32::from(bounds.origin.y));
        self.child.prepaint(window, cx);
        if self.reflow.borrow_mut().end_frame() {
            window.request_animation_frame();
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

impl Element for FlipCard {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let now = cx.background_executor().now();
        let shift =
            self.reflow
                .borrow_mut()
                .place_card(&self.task_id, f32::from(bounds.origin.y), now);
        window.with_element_offset(point(px(0.0), px(shift)), |window| {
            self.child.prepaint(window, cx)
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_ease_out_starts_at_zero_and_ends_at_one() {
        assert!(css_ease_out(0.0).abs() < 1e-4);
        assert!((css_ease_out(1.0) - 1.0).abs() < 1e-4);
        // ease-out은 앞쪽에서 빨리 나아간다
        assert!(css_ease_out(0.5) > 0.6);
    }
}
