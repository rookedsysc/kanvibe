//! parity/diagrams/baseline/*.mmd를 모바일 경로(폭 800, 라이트·다크)로 그려 비교용 PNG를 남긴다.
//!
//! `cargo run -p kanvibe-diagram --example render_parity -- <출력 디렉터리> [id ...]`
//! 출력은 `parity/tools/screens/diff.py --dirs parity/diagrams/baseline <출력 디렉터리>`로 비교한다.

use std::path::{Path, PathBuf};

use kanvibe_diagram::raster;
use kanvibe_diagram::text::FontBook;
use kanvibe_diagram::theme::{Palette, Theme};

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
    for (theme, name) in [(Theme::Light, "light"), (Theme::Dark, "dark")] {
        let scene = match kanvibe_diagram::render_scene(definition.trim(), theme, fonts) {
            Ok(scene) => scene,
            Err(error) => {
                println!("{id}--{name}: {error}");
                continue;
            }
        };
        let background = Palette::for_theme(theme).page_background;
        let (pixmap, layout) =
            raster::rasterize_mobile(&scene, fonts, background, PARITY_WIDTH).ok_or("픽스맵")?;
        std::fs::write(
            out_dir.join(format!("{id}--{name}.png")),
            raster::encode_png(&pixmap)?,
        )?;
        println!(
            "{id}--{name}: {PARITY_WIDTH}x{} natural {}",
            layout.image_height, layout.natural_width
        );
    }
    Ok(())
}
