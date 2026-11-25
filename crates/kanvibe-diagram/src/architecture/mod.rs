//! architecture-beta 다이어그램(architectureDb.ts·architectureRenderer.ts·svgDraw.ts).
//!
//! 서비스·접점·그룹을 cytoscape 그래프로 만들고 cytoscape-fcose로 배치한 뒤 간선, 서비스, 그룹 순서로 그린다.
//! 간선의 두 변(`L`·`R`·`T`·`B`)이 격자 칸을 정하고, 그 칸에서 나온 정렬·상대 배치 제약이 fcose를 이끈다.
//! Mermaid는 배치가 끝난 뒤(`layoutstop`) 꺾인 간선 꼴을 맞추고 같은 배치를 한 번 더 돌리므로 여기서도 두 번 돈다.
//! 두 번째 배치는 첫 결과 자리에서 출발하고, 되돌려 놓을 가운데도 그 자리의 경계 상자로 다시 잰다.
//! 크기·여백 등은 `config.architecture`([`config::ArchitectureConfig`])를 따른다.

mod config;
mod cytoscape;
mod draw;
mod fcose;
mod icon_text;
mod icons;
mod parser;
mod placement;

use crate::scene::Scene;
use crate::{DiagramError, RenderContext};

use config::ArchitectureConfig;
use cytoscape::{CyGraph, SegmentWeight};
use fcose::{LayoutEdge, LayoutInput, LayoutNode};
use parser::Architecture;
use placement::RelativePlacement;

/// 그룹 경계를 넘는 간선은 짧고(0.5 × iconSize) 느슨하게 둔다(fcose issue #67 우회)
const CROSS_GROUP_IDEAL_LENGTH_RATIO: f64 = 0.5;
const CROSS_GROUP_ELASTICITY: f64 = 1e-3;
/// 이웃 칸 사이 상대 배치 간격(× iconSize)
const RELATIVE_PLACEMENT_GAP_RATIO: f64 = 1.5;

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let architecture = parser::parse(definition)?;
    let config = ArchitectureConfig::from_definition(definition);
    let graph = CyGraph::new(&architecture, context.fonts, config);
    let arrangement = arrange(&architecture, &graph);
    Ok(draw::draw(&architecture, &graph, &arrangement, context))
}

/// 배치 결과: cytoscape 노드 위치(묶음이 아닌 노드의 중심, Mermaid는 이 점을 서비스 아이콘의 왼쪽 위로 쓴다)와
/// 간선마다 꺾는 점 무게·거리
pub(crate) struct Arrangement {
    pub centers: Vec<(f64, f64)>,
    pub segments: Vec<SegmentWeight>,
}

/// `layout.run()` → layoutstop(꺾는 점 다시 잡기) → `layout.run()`
fn arrange(architecture: &Architecture, graph: &CyGraph) -> Arrangement {
    let input = layout_input(architecture, graph);
    let mut arrangement = Arrangement {
        centers: vec![(0.0, 0.0); graph.nodes.len()],
        segments: vec![SegmentWeight::INITIAL; graph.edges.len()],
    };
    run_layout(graph, input.clone(), &mut arrangement);
    arrangement.segments = graph.segment_weights(&arrangement.centers, &arrangement.segments);
    run_layout(graph, input, &mut arrangement);
    arrangement
}

fn run_layout(graph: &CyGraph, mut input: LayoutInput, arrangement: &mut Arrangement) {
    let original_center = graph.elements_center(&arrangement.centers, &arrangement.segments);
    for (node, center) in input.nodes.iter_mut().zip(&arrangement.centers) {
        node.center = *center;
    }
    let boxes = fcose::run(&input, original_center);
    for (node, laid_out) in boxes.iter().enumerate() {
        if !graph.is_compound(node) {
            arrangement.centers[node] = laid_out.center;
        }
    }
}

/// cytoscape 노드·간선과 Mermaid가 넘기는 fcose 선택지(이상 길이·탄성·제약)
fn layout_input(architecture: &Architecture, graph: &CyGraph) -> LayoutInput {
    let nodes = (0..graph.nodes.len())
        .map(|node| {
            let size = graph.leaf_size(node);
            LayoutNode {
                parent: graph.nodes[node].parent,
                center: (0.0, 0.0),
                width: size,
                height: size,
                padding: graph.config.padding,
            }
        })
        .collect();
    let edges = graph
        .edges
        .iter()
        .map(|edge| layout_edge(graph, edge.source, edge.target))
        .collect();
    let constraints = placement::placement_constraints(architecture);
    let gap = RELATIVE_PLACEMENT_GAP_RATIO * graph.config.icon_size;
    let to_cy = |nodes: &Vec<usize>| -> Vec<usize> {
        nodes.iter().map(|&node| graph.node_to_cy[node]).collect()
    };
    let relative = constraints
        .relative
        .iter()
        .map(|placement| (to_cy_placement(*placement, &graph.node_to_cy), gap))
        .collect();
    LayoutInput {
        nodes,
        edges,
        horizontal: constraints.horizontal.iter().map(to_cy).collect(),
        vertical: constraints.vertical.iter().map(to_cy).collect(),
        relative,
        max_iterations: graph.config.iterations,
    }
}

/// idealEdgeLength·edgeElasticity: 두 끝의 부모(그룹)가 같은지로 고른다
fn layout_edge(graph: &CyGraph, source: usize, target: usize) -> LayoutEdge {
    let is_same_parent = graph.nodes[source].parent == graph.nodes[target].parent;
    let config = &graph.config;
    let (ideal_length, elasticity) = if is_same_parent {
        (
            config.ideal_edge_length_multiplier * config.icon_size,
            config.edge_elasticity,
        )
    } else {
        (
            CROSS_GROUP_IDEAL_LENGTH_RATIO * config.icon_size,
            CROSS_GROUP_ELASTICITY,
        )
    };
    LayoutEdge {
        source,
        target,
        ideal_length,
        elasticity,
    }
}

fn to_cy_placement(placement: RelativePlacement, node_to_cy: &[usize]) -> RelativePlacement {
    match placement {
        RelativePlacement::Horizontal { left, right } => RelativePlacement::Horizontal {
            left: node_to_cy[left],
            right: node_to_cy[right],
        },
        RelativePlacement::Vertical { top, bottom } => RelativePlacement::Vertical {
            top: node_to_cy[top],
            bottom: node_to_cy[bottom],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    use crate::LabelMode;
    use crate::theme::Palette;
    use crate::theme::Theme;

    fn baseline_definition(case: &str) -> String {
        let path = format!(
            "{}/../../parity/diagrams/baseline/architecture-{case}.mmd",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).expect("기준 정의")
    }

    struct Laid {
        architecture: Architecture,
        graph: CyGraph,
        centers: Vec<(f64, f64)>,
        segments: Vec<SegmentWeight>,
    }

    fn lay_out(case: &str) -> Laid {
        let fonts = bundled_fonts();
        let definition = baseline_definition(case);
        let architecture = parser::parse(&definition).unwrap();
        let config = ArchitectureConfig::from_definition(&definition);
        let graph = CyGraph::new(&architecture, &fonts, config);
        let Arrangement { centers, segments } = arrange(&architecture, &graph);
        Laid {
            architecture,
            graph,
            centers,
            segments,
        }
    }

    fn assert_close(actual: (f64, f64), expected: (f64, f64)) {
        assert_within(actual, expected, 1e-6);
    }

    fn assert_within(actual: (f64, f64), expected: (f64, f64), tolerance: f64) {
        let error = (actual.0 - expected.0)
            .abs()
            .max((actual.1 - expected.1).abs());
        assert!(error < tolerance, "{actual:?} != {expected:?}");
    }

    impl Laid {
        fn center(&self, id: &str) -> (f64, f64) {
            let index = self.architecture.node_index(id).expect("노드");
            self.centers[self.graph.node_to_cy[index]]
        }

        /// 기준 SVG 간선 경로의 가운데 점(`L` 다음 점)
        fn midpoint(&self, edge: usize) -> (f64, f64) {
            let cy_edge = &self.graph.edges[edge];
            self.graph
                .edge_midpoint(cy_edge, &self.centers, self.segments[edge])
        }

        /// 기준 SVG `<rect id="…-group-…">`의 (x, y, width, height)
        fn group_rect(&self, id: &str) -> (f64, f64, f64, f64) {
            let position = self
                .graph
                .nodes
                .iter()
                .position(|node| {
                    matches!(node.element, cytoscape::CyElement::Group(index)
                        if self.architecture.groups[index].id == id)
                })
                .expect("그룹");
            let rects = self.graph.outer_rects(&self.centers);
            let bounds = self
                .graph
                .node_bounding_box(position, &rects, &self.centers);
            let half = self.graph.config.icon_size / 2.0;
            (
                bounds.x1 + half,
                bounds.y1 + half,
                bounds.width(),
                bounds.height(),
            )
        }
    }

    #[test]
    fn 기본_예제_배치가_기준_svg_좌표와_같다() {
        let laid = lay_out("basic");
        // 두 번 돈 fcose 결과(서비스 transform)
        assert_close(
            laid.center("db"),
            (100.913_575_517_482_37, -83.461_316_778_892_35),
        );
        assert_close(
            laid.center("disk1"),
            (-100.913_575_517_482_37, 117.461_316_778_892_38),
        );
        let (x, y, width, height) = laid.group_rect("api");
        assert_close((x, y), (-143.413_575_517_482_37, -125.961_316_778_892_35));
        assert_close(
            (width, height),
            (366.827_151_034_964_74, 382.922_633_557_784_7),
        );
    }

    #[test]
    fn 그룹_사이_간선과_접점이_있는_배치가_기준_svg와_같다() {
        let laid = lay_out("junctions");
        assert_close(
            laid.center("gateway"),
            (-241.606_927_247_649_08, 16.943_335_840_014_544),
        );
        assert_close(
            laid.center("api"),
            (40.457_209_364_057_654, 16.943_335_840_014_544),
        );
        assert_close(
            laid.center("db"),
            (241.606_927_247_649_1, -183.924_865_956_634_8),
        );
        assert_close(
            laid.center("hub"),
            (241.606_927_247_649_1, 16.943_335_840_014_544),
        );
        let (x, _, width, height) = laid.group_rect("core");
        assert_close(
            (x, width),
            (-2.042_790_635_942_367_3, 366.149_717_883_591_45),
        );
        assert!((height - 583.849_731_913_269_7).abs() < 1e-6);
    }

    #[test]
    fn 중첩_그룹과_그룹_밖_노드_배치가_기준_svg와_같다() {
        let laid = lay_out("icons");
        assert_close(
            laid.center("web"),
            (-172.977_093_125_762_82, -206.243_290_899_878_84),
        );
        assert_close(
            laid.center("store"),
            (38.179_429_189_014_62, 68.158_527_588_201_64),
        );
        assert_close(
            laid.center("lambda"),
            (251.477_093_125_762_7, 279.743_290_899_878_84),
        );
        let (x, y, width, height) = laid.group_rect("inner");
        assert_close((x, y), (-215.477_093_125_762_85, 25.658_527_588_201_636));
        assert_close((width, height), (376.156_522_314_777_5, 182.0));
        let (x, y, width, height) = laid.group_rect("outer");
        assert_close((x, y), (-256.977_093_125_762_85, -248.743_290_899_878_84));
        assert_close(
            (width, height),
            (459.156_522_314_777_5, 497.901_818_488_080_5),
        );
    }

    #[test]
    fn viewbox는_그린_것_전체에_여백_40을_두른다() {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::SvgText,
        };
        // 기준 SVG viewBox: 기본 예제 446.8271484375 × 462.922607421875, 아이콘 예제는 아래 라벨이 높이를 정한다
        let scene = render(&baseline_definition("basic"), &context).unwrap();
        assert_close(
            (scene.width, scene.height),
            (446.827_148_437_5, 462.922_607_421_875),
        );
        let scene = render(&baseline_definition("icons"), &context).unwrap();
        assert!(
            (scene.width - 668.454_223_632_812_5).abs() < 0.01,
            "{}",
            scene.width
        );
        assert!(
            (scene.height - 713.767_822_265_625).abs() < 0.05,
            "{}",
            scene.height
        );
    }

    #[test]
    fn 꺾인_간선의_꺾는_점은_첫_배치_모서리를_두_번째_배치로_옮긴_점이다() {
        let laid = lay_out("bends");
        // 이 그래프는 fcose의 Math.random 섞기에 따라 TS 결과가 갈린다. 최종 노드 자리는 두 갈래(약 2e-5px)로,
        // headless cytoscape 12회 중 5회가 기준 이미지, 7회가 이 구현(섞지 않음)과 같았다. 꺾는 점은 첫 배치
        // 결과로 정해져 첫 배치의 갈래(약 0.3px)만큼 0.02px 안팎으로 흔들린다
        let assert_close = |actual, expected| assert_within(actual, expected, 0.05);
        assert_close(
            laid.center("api"),
            (65.563_185_149_329_88, -62.296_139_574_549_46),
        );
        assert_close(
            laid.center("cache"),
            (-21.412_285_558_432_146, 31.766_210_516_601_35),
        );
        // web:R -[https]- T:api, cache:B -[warm]- R:db
        assert_close(
            laid.midpoint(0),
            (105.405_952_619_727_2, 170.704_325_113_490_68),
        );
        assert_close(
            laid.midpoint(2),
            (18.591_082_815_744_702, 170.657_503_879_404_71),
        );
        // api:R -[jobs]- T:queue: 두 중심이 세로로 나란해 처음 무게(0)·거리(0.5) 그대로다
        assert_close(
            laid.midpoint(3),
            (145.079_454_695_198_6, -22.422_649_841_903_294),
        );
    }

    #[test]
    fn architecture_설정의_아이콘_크기와_여백을_따른다() {
        let laid = lay_out("config");
        assert_close(
            laid.center("front"),
            (-161.911_770_853_043_27, -55.014_609_339_752_894),
        );
        assert_close(
            laid.center("store"),
            (190.411_770_853_043_27, 97.014_609_339_752_97),
        );
        let (x, y, width, height) = laid.group_rect("app");
        assert_close((x, y), (-194.411_770_853_043_27, -87.514_609_339_752_9));
        assert_close(
            (width, height),
            (276.625_198_502_388_46, 298.029_218_679_505_85),
        );
    }
}
