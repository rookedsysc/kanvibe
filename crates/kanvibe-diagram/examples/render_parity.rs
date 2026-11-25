//! parity/diagrams/baseline/*.mmd를 모바일 경로(폭 800, 라이트·다크)로 그려 비교용 PNG를 남긴다.
//!
//! `cargo run -p kanvibe-diagram --example render_parity -- <출력 디렉터리> [id ...]`
//! 모바일 경로(HTML 라벨, `themeVariables` 막음)는 모바일 계약의 JPEG를 거쳐 `<id>--<theme>.png`, 데스크톱 경로(SVG 글자
//! 라벨, `themeVariables` 씀)는 손실 없이 `<id>--desktop--<theme>.png`로 남긴다. 기준 이미지도 같은 방식으로 찍는다
//! (모바일은 TS 워커 JPEG, 데스크톱은 같은 프레임의 PNG).
//! 출력은 `parity/tools/screens/diff.py --dirs parity/diagrams/baseline <출력 디렉터리>`로 비교한다.

use std::path::{Path, PathBuf};

use kanvibe_diagram::LabelMode;
use kanvibe_diagram::raster;
use kanvibe_diagram::text::FontBook;
use kanvibe_diagram::theme::{Palette, Theme};
use kanvibe_diagram::theme_variables::ThemeVariablesPolicy;

const PARITY_WIDTH: u32 = 800;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let out_dir = PathBuf::from(arguments.next().ok_or("출력 디렉터리가 필요합니다")?);
    let only: Vec<String> = arguments.collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fonts = FontBook::load_dir(&root.join("crates/kanvibe-app/assets/fonts"))?;
    std::fs::create_dir_all(&out_dir)?;
    let mut sources: Vec<PathBuf> = std::fs::read_dir(root.join("parity/diagrams/baseline"))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|file_path| {
            file_path
                .extension()
                .is_some_and(|extension| extension == "mmd")
        })
        .collect();
    sources.sort();
    for source in sources {
        let id = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        if only.is_empty() || only.contains(&id) {
            render_source(&fonts, &source, &id, &out_dir)?;
        }
    }
    Ok(())
}

fn render_source(
    fonts: &FontBook,
    source: &Path,
    id: &str,
    out_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let definition = std::fs::read_to_string(source)?;
    let (mobile, desktop) = (ThemeVariablesPolicy::Ignore, ThemeVariablesPolicy::Apply);
    let variants = [
        (LabelMode::Html, mobile, Theme::Light, "light"),
        (LabelMode::Html, mobile, Theme::Dark, "dark"),
        (LabelMode::SvgText, desktop, Theme::Light, "desktop--light"),
        (LabelMode::SvgText, desktop, Theme::Dark, "desktop--dark"),
    ];
    for (mode, policy, theme, name) in variants {
        let scene =
            match kanvibe_diagram::render_scene(definition.trim(), theme, mode, policy, fonts) {
                Ok(scene) => scene,
                Err(error) => {
                    println!("{id}--{name}: {error}");
                    continue;
                }
            };
        let background = Palette::for_theme(theme).page_background;
        let (pixmap, layout) =
            raster::rasterize_mobile(&scene, fonts, background, PARITY_WIDTH).ok_or("픽스맵")?;
        let png = match policy {
            ThemeVariablesPolicy::Ignore => jpeg_round_trip(&pixmap)?,
            ThemeVariablesPolicy::Apply => raster::encode_png(&pixmap)?,
        };
        std::fs::write(out_dir.join(format!("{id}--{name}.png")), png)?;
        println!(
            "{id}--{name}: {PARITY_WIDTH}x{} natural {}",
            layout.image_height, layout.natural_width
        );
    }
    Ok(())
}

/// 모바일 계약이 실제로 보내는 JPEG(`raster::encode_jpeg`)를 한 번 거친 그림. 기준 이미지도 TS 워커의 JPEG라
/// JPEG끼리 견준다
fn jpeg_round_trip(pixmap: &tiny_skia::Pixmap) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let jpeg = raster::encode_jpeg(pixmap)?;
    let decoded = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)?.to_rgb8();
    let mut png = Vec::new();
    decoded.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok(png)
}
