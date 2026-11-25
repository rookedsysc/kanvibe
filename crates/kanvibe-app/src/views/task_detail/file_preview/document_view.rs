//! 파일 미리보기의 문서 보기(TS `<iframe sandbox="allow-scripts" srcdoc=… class="h-full w-full border-0 bg-white">`).
//!
//! SVG는 앱 안에서 resvg로 그린다(gpui-pre가 쓰는 것과 같은 판). 크기가 적힌 SVG는 그 크기로 왼쪽 위에, 크기가 없으면
//! 브라우저처럼 틀을 채우되 비율을 지켜 가운데에 둔다. 바깥 파일·주소를 가리키는 그림은 읽지 않는다(TS 정책 `img-src data:`).
//!
//! HTML은 GPUI Kit의 실험적 WebView(wry, Linux는 WebKitGTK)로 그린다. TS 틀과 같게 스크립트는 돌지만 출처는
//! 불투명(null)이고, 처음 문서 말고는 어디로도 이동하지 않으며, 새 창과 개발자 도구를 막고, 문서 맨 앞의 정책이
//! 바깥으로 나가는 요청과 폼 제출을 막는다. Linux의 WebView는 X11 창에서만 만들 수 있어 앱은 Linux에서 X11로 뜬다
//! (`main.rs`). 그래도 만들지 못하면 TS iframe에 대응하는 안내 문구가 없으므로 빈 흰 틀만 두고 이유는 로그에 남긴다. gpui-pre에서 WebView는 창 위에 얹힌 네이티브 창이라 그 자리의 GPUI 팝오버·대화상자를 가리므로,
//! 다른 곳이 키 입력을 가져가면(대화상자·팔레트가 열리면) 숨긴다.

use std::sync::{Arc, OnceLock};

use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, Image, ImageFormat, IntoElement, ObjectFit,
    ParentElement, Styled, StyledImage, Task, Window, div, img, px,
};
use gpui_webview::WebView;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

use crate::fonts::{BUNDLED_FONT_FILES, MONO_FAMILY, SANS_FAMILY};

/// TS `PREVIEW_CONTENT_SECURITY_POLICY`
pub const PREVIEW_CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; form-action 'none'";
/// 크기 없는 SVG를 틀에 맞출 때 그려 두는 긴 변 길이(px)
const FITTED_SVG_PIXELS: f32 = 2048.0;
/// WebView가 처음 읽는 문서 주소(`load_html`의 기본 주소)
const INITIAL_DOCUMENT_URL: &str = "about:blank";

/// 문서가 스스로 실행을 시작하기 전에 정책이 먼저 읽히도록 맨 앞에 넣는다(TS `buildSandboxedPreviewDocument`).
/// doctype 앞에 다른 것이 오면 문서가 호환 모드로 그려지므로, doctype이 있으면 그 바로 뒤에 넣는다
pub fn sandboxed_preview_document(content: &str) -> String {
    let policy = format!(
        r#"<meta http-equiv="Content-Security-Policy" content="{PREVIEW_CONTENT_SECURITY_POLICY}">"#
    );
    let doctype_end = leading_doctype_end(content).unwrap_or(0);
    format!(
        "{}{policy}{}",
        &content[..doctype_end],
        &content[doctype_end..]
    )
}

/// `/^\s*<!doctype[^>]*>/i`가 맞는 끝 위치
fn leading_doctype_end(content: &str) -> Option<usize> {
    let start = content.len() - content.trim_start().len();
    let rest = &content[start..];
    let prefix = rest.get(..9)?;
    if !prefix.eq_ignore_ascii_case("<!doctype") {
        return None;
    }
    rest.find('>').map(|close| start + close + 1)
}

/// 처음 문서(같은 문서 안 조각 이동 포함)만 허용한다. 바깥 주소·`file://`·`data:` 이동은 막는다
pub fn is_allowed_navigation(url: &str) -> bool {
    url == INITIAL_DOCUMENT_URL || url.starts_with("about:blank#")
}

/// 그린 SVG. `natural_size`가 없으면 틀에 맞춘다
#[derive(Clone)]
pub struct RenderedSvg {
    pub image: Arc<Image>,
    pub natural_size: Option<(f32, f32)>,
}

/// SVG 보기 상태: 그리는 중·그림·실패
#[derive(Clone)]
pub enum SvgRender {
    Pending,
    Ready(RenderedSvg),
    Failed,
}

/// HTML 보기 상태
pub enum HtmlRender {
    Ready(Entity<WebView>),
    /// WebView를 만들지 못했다(이유는 로그에 남겼다)
    Unavailable,
}

/// 파일 하나의 문서 보기
pub enum DocumentPreview {
    Svg {
        render: SvgRender,
        _task: Option<Task<()>>,
    },
    Html(HtmlRender),
}

fn svg_font_database() -> Arc<usvg::fontdb::Database> {
    static DATABASE: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DATABASE
        .get_or_init(|| {
            let mut database = usvg::fontdb::Database::new();
            for bytes in BUNDLED_FONT_FILES {
                database.load_font_source(usvg::fontdb::Source::Binary(Arc::new(bytes)));
            }
            database.set_sans_serif_family(SANS_FAMILY);
            database.set_serif_family(SANS_FAMILY);
            database.set_monospace_family(MONO_FAMILY);
            Arc::new(database)
        })
        .clone()
}

/// 바깥 파일·주소 그림은 읽지 않고 data URL 그림만 쓴다
fn sandboxed_svg_options() -> usvg::Options<'static> {
    usvg::Options {
        font_family: SANS_FAMILY.to_string(),
        fontdb: svg_font_database(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

/// 맨 바깥 `<svg>`에 너비와 높이가 모두 적혔는지
fn has_explicit_size(source: &str) -> bool {
    let Ok(document) = usvg::roxmltree::Document::parse(source) else {
        return false;
    };
    let root = document.root_element();
    root.attribute("width").is_some() && root.attribute("height").is_some()
}

/// SVG 원문을 화면 배율로 래스터화한다. 읽을 수 없는 SVG면 `None`
pub fn rasterize_svg(source: &str, scale_factor: f32) -> Option<RenderedSvg> {
    let tree = usvg::Tree::from_str(source, &sandboxed_svg_options()).ok()?;
    let size = tree.size();
    let natural_size = has_explicit_size(source).then(|| (size.width(), size.height()));
    let scale = match natural_size {
        Some(_) => scale_factor.max(1.0),
        None => FITTED_SVG_PIXELS / size.width().max(size.height()),
    };
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().ok()?;
    Some(RenderedSvg {
        image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        natural_size,
    })
}

impl DocumentPreview {
    /// SVG를 백그라운드에서 그리기 시작한다. `on_ready`는 결과를 상태에 넣는다
    pub fn start_svg<V: 'static>(
        source: String,
        scale_factor: f32,
        cx: &mut Context<V>,
        on_ready: impl FnOnce(&mut V, SvgRender) + 'static,
    ) -> Self {
        let rasterize = cx.background_spawn(async move { rasterize_svg(&source, scale_factor) });
        let task = cx.spawn(async move |this, cx| {
            let render = match rasterize.await {
                Some(svg) => SvgRender::Ready(svg),
                None => SvgRender::Failed,
            };
            let _ = this.update(cx, |view, cx| {
                on_ready(view, render);
                cx.notify();
            });
        });
        Self::Svg {
            render: SvgRender::Pending,
            _task: Some(task),
        }
    }

    /// HTML을 격리한 WebView로 연다
    pub fn open_html(content: &str, window: &mut Window, cx: &mut App) -> Self {
        let builder = wry::WebViewBuilder::new()
            .with_html(sandboxed_preview_document(content))
            .with_navigation_handler(|url| is_allowed_navigation(&url))
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_devtools(false)
            .with_incognito(true);
        match WebView::build(builder, window, cx) {
            Ok(webview) => Self::Html(HtmlRender::Ready(cx.new(|_| webview))),
            Err(error) => {
                eprintln!("[kanvibe] HTML preview WebView unavailable: {error}");
                Self::Html(HtmlRender::Unavailable)
            }
        }
    }
}

/// 문서 보기 요소. `bg-white`를 깐 틀을 가득 채운다
pub fn render_document(preview: &DocumentPreview) -> AnyElement {
    let frame = div().size_full().bg(gpui_kit::white());
    match preview {
        DocumentPreview::Svg { render, .. } => match render {
            SvgRender::Pending => frame.into_any_element(),
            SvgRender::Failed => frame.into_any_element(),
            SvgRender::Ready(svg) => frame.child(svg_image(svg)).into_any_element(),
        },
        DocumentPreview::Html(HtmlRender::Ready(webview)) => {
            frame.child(webview.clone()).into_any_element()
        }
        DocumentPreview::Html(HtmlRender::Unavailable) => frame.into_any_element(),
    }
}

fn svg_image(svg: &RenderedSvg) -> AnyElement {
    let image = img(svg.image.clone());
    match svg.natural_size {
        Some((width, height)) => image
            .flex_none()
            .w(px(width))
            .h(px(height))
            .into_any_element(),
        None => image
            .size_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; form-action 'none'">"#;

    // parity: TS-renderer-AiChatFilePreview-76ec05
    #[test]
    fn the_request_blocking_policy_goes_right_after_a_doctype_or_first() {
        assert_eq!(
            sandboxed_preview_document("<!DOCTYPE html><html><script>run()</script></html>"),
            format!("<!DOCTYPE html>{POLICY}<html><script>run()</script></html>")
        );
        assert_eq!(
            sandboxed_preview_document("\n <!doctype html>\n<p>a</p>"),
            format!("\n <!doctype html>{POLICY}\n<p>a</p>")
        );
        assert_eq!(
            sandboxed_preview_document("<script>run()</script><p>a</p>"),
            format!("{POLICY}<script>run()</script><p>a</p>")
        );
        assert_eq!(sandboxed_preview_document(""), POLICY);
    }

    // parity: TS-renderer-AiChatFilePreview-3b528a
    #[test]
    fn html_runs_scripts_in_an_isolated_document_that_cannot_leave() {
        // 문서는 주소 없이 원문으로 읽고(불투명 출처), 처음 문서 안에서만 움직인다
        let document =
            sandboxed_preview_document("<!doctype html><script>document.title = 1</script>");
        assert_eq!(
            document,
            format!("<!doctype html>{POLICY}<script>document.title = 1</script>")
        );
        assert!(is_allowed_navigation("about:blank"));
        assert!(is_allowed_navigation("about:blank#section"));
        for denied in [
            "https://example.com/",
            "file:///etc/passwd",
            "data:text/html,<p>x</p>",
            "about:srcdoc",
        ] {
            assert!(!is_allowed_navigation(denied), "{denied}");
        }
    }

    // parity: TS-main-embeddedDocumentNavigation-767f25
    #[test]
    fn an_embedded_document_cannot_move_itself_to_another_address() {
        // 앱 창 자체는 GPUI가 그려 이 규칙과 무관하고, 창 안에 넣은 문서만 처음 문서에 묶인다
        assert!(is_allowed_navigation(INITIAL_DOCUMENT_URL));
        for redirect in [
            "https://attacker.example/login",
            "http://localhost:9736/",
            "kanvibe://task/1",
            "javascript:location='https://example.com'",
        ] {
            assert!(!is_allowed_navigation(redirect), "{redirect}");
        }
    }

    #[test]
    fn sized_svgs_keep_their_size_and_unsized_ones_fit_the_frame() {
        let sized = rasterize_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="160"><rect width="10" height="10"/></svg>"#,
            1.0,
        )
        .expect("svg");
        assert_eq!(sized.natural_size, Some((320.0, 160.0)));
        let fitted = rasterize_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 20"><rect width="10" height="10"/></svg>"#,
            1.0,
        )
        .expect("svg");
        assert_eq!(fitted.natural_size, None);
        assert!(rasterize_svg("<not-svg", 1.0).is_none());
    }
}
