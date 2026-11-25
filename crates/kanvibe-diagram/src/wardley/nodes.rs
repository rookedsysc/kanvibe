//! 노드 그림: 조달 전략 겹원(build·buy·outsource·market), 본 원, 시장 삼각형, 파이프라인 부모 네모,
//! 관성 막대, 이름 글자. 한 노드의 그림은 이 순서로 겹친다.

use crate::scene::{Color, Point, polygon};
use crate::text::FontWeight;

use super::layers::SQUARE_SIZE;
use super::parser::{MapNode, NodeKind, SourcingStrategy, WardleyMap};
use super::{Anchor, Baseline, Painter, TextStyle};

const NODE_RADIUS: f64 = 6.0;
const LABEL_OFFSET: f64 = 8.0;
const LABEL_FONT_SIZE: f64 = 10.0;
const WHITE: Color = Color::rgb(255, 255, 255);
const BLACK: Color = Color::rgb(0, 0, 0);

pub fn draw_nodes(painter: &mut Painter, map: &WardleyMap, positions: &[Point]) {
    for (node, position) in map.nodes.iter().zip(positions) {
        draw_strategy_overlay(painter, node, *position);
        let is_plain_circle = !node.is_pipeline_parent
            && node.strategy != Some(SourcingStrategy::Market)
            && node.kind != NodeKind::Anchor;
        if is_plain_circle {
            let colors = painter.colors;
            painter.circle(
                *position,
                NODE_RADIUS,
                colors.component_fill,
                colors.component_stroke,
                1.0,
            );
        }
        if node.strategy == Some(SourcingStrategy::Market) {
            draw_market_triangle(painter, *position);
        }
        if node.is_pipeline_parent {
            draw_parent_square(painter, *position);
        }
        if node.has_inertia {
            draw_inertia(painter, node, *position);
        }
        draw_label(painter, node, *position);
    }
}

/// 전략 겹원(반지름 12). build만 테두리가 검정이다
fn draw_strategy_overlay(painter: &mut Painter, node: &MapNode, position: Point) {
    let stroke = painter.colors.component_stroke;
    let (fill, stroke) = match node.strategy {
        None => return,
        Some(SourcingStrategy::Outsource) => (Color::rgb(0x66, 0x66, 0x66), stroke),
        Some(SourcingStrategy::Buy) => (Color::rgb(0xCC, 0xCC, 0xCC), stroke),
        Some(SourcingStrategy::Build) => (Color::rgb(0xEE, 0xEE, 0xEE), BLACK),
        Some(SourcingStrategy::Market) => (WHITE, stroke),
    };
    painter.circle(position, NODE_RADIUS * 2.0, fill, stroke, 1.0);
}

/// 시장(market): 반지름 7.2 삼각형 꼭짓점을 잇는 선과 꼭짓점마다 작은 흰 원
fn draw_market_triangle(painter: &mut Painter, position: Point) {
    let radius = NODE_RADIUS * 1.2;
    let (cos, sin) = (
        (std::f64::consts::PI / 6.0).cos(),
        (std::f64::consts::PI / 6.0).sin(),
    );
    let corners = [
        Point::new(position.x, position.y - radius),
        Point::new(position.x - radius * cos, position.y + radius * sin),
        Point::new(position.x + radius * cos, position.y + radius * sin),
    ];
    let stroke = painter.colors.component_stroke;
    for (from, to) in [(0, 1), (1, 2), (2, 0)] {
        painter.line(corners[from], corners[to], stroke, 1.0, &[]);
    }
    for corner in corners {
        painter.circle(corner, NODE_RADIUS * 0.7, WHITE, stroke, 2.0);
    }
}

fn draw_parent_square(painter: &mut Painter, position: Point) {
    let half = SQUARE_SIZE / 2.0;
    let corners = [
        Point::new(position.x - half, position.y - half),
        Point::new(position.x + half, position.y - half),
        Point::new(position.x + half, position.y + half),
        Point::new(position.x - half, position.y + half),
    ];
    let colors = painter.colors;
    let stroke = super::Painter::stroke(colors.component_stroke, 1.0, &[]);
    painter
        .scene
        .push_path(polygon(&corners), Some(colors.component_fill), stroke);
}

/// 관성: 노드 오른쪽의 굵기 6 세로 막대. 전략 겹원이 있으면 더 비켜 놓는다
fn draw_inertia(painter: &mut Painter, node: &MapNode, position: Point) {
    let (mut offset, length) = match node.is_pipeline_parent {
        true => (SQUARE_SIZE / 2.0 + 15.0, SQUARE_SIZE),
        false => (NODE_RADIUS + 15.0, NODE_RADIUS * 2.0),
    };
    if node.strategy.is_some() {
        offset += NODE_RADIUS + 10.0;
    }
    let x = position.x + offset;
    let stroke = painter.colors.component_stroke;
    painter.line(
        Point::new(x, position.y - length / 2.0),
        Point::new(x, position.y + length / 2.0),
        stroke,
        6.0,
        &[],
    );
}

/// 이름: 닻(anchor)은 굵은 검정 글자를 가운데에, 나머지는 오른쪽 위(+8, −8)에 둔다
fn draw_label(painter: &mut Painter, node: &MapNode, position: Point) {
    if node.kind == NodeKind::Anchor {
        let at = Point::new(
            position.x + node.label_offset_x.unwrap_or(0.0),
            position.y + node.label_offset_y.unwrap_or(-3.0),
        );
        let style = TextStyle {
            font_size: LABEL_FONT_SIZE,
            weight: FontWeight::Bold,
            anchor: Anchor::Middle,
            baseline: Baseline::Middle,
            color: BLACK,
        };
        painter.text(&node.label, at, style, None);
        return;
    }
    let strategy_shift = if node.strategy.is_some() { 10.0 } else { 0.0 };
    let offset_x = node.label_offset_x.unwrap_or(LABEL_OFFSET + strategy_shift);
    let offset_y = node
        .label_offset_y
        .unwrap_or(-LABEL_OFFSET - strategy_shift);
    let style = TextStyle {
        font_size: LABEL_FONT_SIZE,
        weight: FontWeight::Regular,
        anchor: Anchor::Start,
        baseline: Baseline::Alphabetic,
        color: painter.colors.component_label,
    };
    let at = Point::new(position.x + offset_x, position.y + offset_y);
    painter.text(&node.label, at, style, None);
}
