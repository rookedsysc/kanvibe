//! block/layout.ts: 격자 배치.
//!
//! 자식들 가운데 칸 하나당 가장 넓은 폭·가장 큰 높이로 모든 자식 크기를 맞추고(여러 칸 블록은 칸 수만큼과 사이
//! 여백), 묶음은 자식 격자에 여백을 두른 크기가 된다. 형제보다 좁은 묶음은 형제 크기로 늘리고 자식을 다시 나눈다.
//! 위치는 왼쪽부터 칸을 채우며 줄마다 가장 큰 자식 높이로 줄 간격을 정한다.

use std::collections::BTreeMap;

use super::{Block, BlockSize, BlockType};

/// block.padding 기본값
pub(crate) const PADDING: f64 = 8.0;

/// calculateBlockPosition: 칸 수가 음수(auto)면 한 줄, 1이면 한 열
fn grid_position(columns: i64, position: i64) -> (i64, i64) {
    if columns < 0 {
        return (position, 0);
    }
    if columns == 1 {
        return (0, position);
    }
    (position % columns, position / columns)
}

fn size_of(blocks: &[Block], index: usize) -> BlockSize {
    blocks[index].size.unwrap_or_default()
}

/// getMaxChildSize: 빈칸은 빼고 칸 하나당 폭과 높이의 최댓값
fn max_child_size(blocks: &[Block], index: usize) -> (f64, f64) {
    let mut max_width: f64 = 0.0;
    let mut max_height: f64 = 0.0;
    for &child in &blocks[index].children {
        if blocks[child].kind == BlockType::Space {
            continue;
        }
        let size = size_of(blocks, child);
        max_width = max_width.max(size.width / f64::from(blocks[child].width_in_columns));
        max_height = max_height.max(size.height);
    }
    (max_width, max_height)
}

/// 자식 크기를 칸 하나 크기에 맞춘다
fn fit_children(blocks: &mut [Block], index: usize, max_width: f64, max_height: f64) {
    for child in blocks[index].children.clone() {
        let span = f64::from(blocks[child].width_in_columns);
        blocks[child].size = Some(BlockSize {
            width: max_width * span + PADDING * (span - 1.0),
            height: max_height,
            x: 0.0,
            y: 0.0,
        });
    }
}

fn set_children_size(blocks: &mut [Block], index: usize, width: f64, height: Option<f64>) {
    for child in blocks[index].children.clone() {
        let size = blocks[child].size.get_or_insert_with(BlockSize::default);
        size.width = width;
        if let Some(height) = height {
            size.height = height;
            size.x = 0.0;
            size.y = 0.0;
        }
    }
}

/// setBlockSizes
pub(crate) fn set_block_sizes(
    blocks: &mut [Block],
    index: usize,
    sibling_width: f64,
    sibling_height: f64,
) {
    if blocks[index].size.is_none_or(|size| size.width == 0.0) {
        blocks[index].size = Some(BlockSize {
            width: sibling_width,
            height: sibling_height,
            x: 0.0,
            y: 0.0,
        });
    }
    let children = blocks[index].children.clone();
    if children.is_empty() {
        return;
    }
    for &child in &children {
        set_block_sizes(blocks, child, 0.0, 0.0);
    }
    let (max_width, max_height) = max_child_size(blocks, index);
    fit_children(blocks, index, max_width, max_height);
    for &child in &children {
        set_block_sizes(blocks, child, max_width, max_height);
    }
    let size = grid_size(blocks, index, max_width, max_height);
    let size = grow_to_siblings(blocks, index, size, (sibling_width, sibling_height));
    blocks[index].size = Some(BlockSize {
        width: size.0,
        height: size.1,
        x: 0.0,
        y: 0.0,
    });
}

/// 격자 칸 수(가로, 세로)
fn grid_dimensions(blocks: &[Block], index: usize) -> (f64, f64) {
    let block = &blocks[index];
    let columns = block.columns.unwrap_or(-1);
    let item_count: i64 = block
        .children
        .iter()
        .map(|child| i64::from(blocks[*child].width_in_columns))
        .sum();
    let mut x_size = block.children.len() as i64;
    if columns > 0 && columns < item_count {
        x_size = columns;
    }
    let y_size = (item_count as f64 / x_size as f64).ceil();
    (x_size as f64, y_size)
}

fn grid_size(blocks: &[Block], index: usize, max_width: f64, max_height: f64) -> (f64, f64) {
    let (x_size, y_size) = grid_dimensions(blocks, index);
    (
        x_size * (max_width + PADDING) + PADDING,
        y_size * (max_height + PADDING) + PADDING,
    )
}

/// 형제 칸보다 좁으면 형제 크기로, 이미 정해진 폭보다 좁으면 그 폭으로 늘리고 자식을 다시 나눈다
fn grow_to_siblings(
    blocks: &mut [Block],
    index: usize,
    size: (f64, f64),
    sibling: (f64, f64),
) -> (f64, f64) {
    let (mut width, mut height) = size;
    let (x_size, y_size) = grid_dimensions(blocks, index);
    if width < sibling.0 {
        (width, height) = sibling;
        let child_width = (sibling.0 - x_size * PADDING - PADDING) / x_size;
        let child_height = (sibling.1 - y_size * PADDING - PADDING) / y_size;
        set_children_size(blocks, index, child_width, Some(child_height));
    }
    let current_width = size_of(blocks, index).width;
    if width < current_width {
        width = current_width;
        let columns = blocks[index].columns.unwrap_or(-1);
        let child_count = blocks[index].children.len() as i64;
        let count = if columns > 0 {
            child_count.min(columns)
        } else {
            child_count
        };
        if count > 0 {
            let count = count as f64;
            let child_width = (width - count * PADDING - PADDING) / count;
            set_children_size(blocks, index, child_width, None);
        }
    }
    (width, height)
}

/// 이 묶음 줄의 시작 x. 묶음 중심 x가 0이면 원본처럼 -padding이다
fn row_start_x(size: BlockSize) -> f64 {
    if size.x != 0.0 {
        size.x - size.width / 2.0
    } else {
        -PADDING
    }
}

/// 차지한 칸 수. 줄 끝을 넘는 만큼은 세지 않는다
fn filled_columns(block: &Block, columns: i64, position: i64) -> i64 {
    let span = i64::from(block.width_in_columns);
    if columns > 0 {
        span.min(columns - position % columns)
    } else {
        span
    }
}

/// 줄 번호별 높이(가장 큰 자식)와 줄 위 끝까지의 거리
fn row_metrics(blocks: &[Block], index: usize) -> (BTreeMap<i64, f64>, BTreeMap<i64, f64>) {
    let columns = blocks[index].columns.unwrap_or(-1);
    let mut heights: BTreeMap<i64, f64> = BTreeMap::new();
    let mut position = 0;
    for &child in &blocks[index].children {
        let (_, row) = grid_position(columns, position);
        let height = heights.entry(row).or_insert(0.0);
        *height = height.max(size_of(blocks, child).height);
        position += filled_columns(&blocks[child], columns, position);
    }
    let mut offsets = BTreeMap::new();
    let mut offset = 0.0;
    for (row, height) in &heights {
        offsets.insert(*row, offset);
        offset += height + PADDING;
    }
    (heights, offsets)
}

/// layoutBlocks: 자식 중심 좌표를 정하고 묶음 자식은 그 안을 다시 배치한다
pub(crate) fn layout_blocks(blocks: &mut [Block], index: usize) {
    if blocks[index].children.is_empty() {
        return;
    }
    let columns = blocks[index].columns.unwrap_or(-1);
    let (heights, offsets) = row_metrics(blocks, index);
    let parent = size_of(blocks, index);
    let mut position = 0;
    let mut start_x = row_start_x(parent);
    let mut current_row = 0;
    for child in blocks[index].children.clone() {
        let (_, row) = grid_position(columns, position);
        if row != current_row {
            current_row = row;
            start_x = row_start_x(parent);
        }
        let size = blocks[child].size.get_or_insert_with(BlockSize::default);
        let half_width = size.width / 2.0;
        size.x = start_x + PADDING + half_width;
        start_x = size.x + half_width;
        let row_height = heights.get(&row).copied().unwrap_or(size.height);
        size.y = parent.y - parent.height / 2.0
            + offsets.get(&row).copied().unwrap_or(0.0)
            + row_height / 2.0
            + PADDING;
        layout_blocks(blocks, child);
        position += filled_columns(&blocks[child], columns, position);
    }
}

/// findBounds: 뿌리를 뺀 모든 블록 상자를 (0, 0)에서 시작해 넓힌다
pub(crate) fn find_bounds(blocks: &[Block], index: usize, bounds: &mut [f64; 4]) {
    if index != 0
        && let Some(size) = blocks[index].size
    {
        bounds[0] = bounds[0].min(size.x - size.width / 2.0);
        bounds[1] = bounds[1].min(size.y - size.height / 2.0);
        bounds[2] = bounds[2].max(size.x + size.width / 2.0);
        bounds[3] = bounds[3].max(size.y + size.height / 2.0);
    }
    for &child in &blocks[index].children {
        find_bounds(blocks, child, bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(width: f64, height: f64, span: u32) -> Block {
        Block {
            width_in_columns: span,
            size: Some(BlockSize {
                width,
                height,
                x: 0.0,
                y: 0.0,
            }),
            ..Block::new("leaf", BlockType::Square)
        }
    }

    #[test]
    fn 세_칸_격자에서_여러_칸_블록과_위치를_정한다() {
        // block-shapes 기준 SVG: 칸 폭 103.875, b는 두 칸(215.75), a 중심 (51.9375, -111.875)
        let mut blocks = vec![Block::new("root", BlockType::Composite)];
        blocks[0].columns = Some(3);
        blocks.push(leaf(76.703125, 32.0, 1));
        blocks.push(leaf(72.359375, 32.0, 2));
        blocks.push(leaf(103.875, 103.875, 1));
        blocks[0].children = vec![1, 2, 3];
        set_block_sizes(&mut blocks, 0, 0.0, 0.0);
        layout_blocks(&mut blocks, 0);
        let b = blocks[2].size.unwrap();
        assert_eq!(b.width, 215.75);
        let a = blocks[1].size.unwrap();
        assert_eq!((a.x, a.width), (51.9375, 103.875));
        let root = blocks[0].size.unwrap();
        assert_eq!((root.width, root.height), (343.625, 231.75));
    }
}
