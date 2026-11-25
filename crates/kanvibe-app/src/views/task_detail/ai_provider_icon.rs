//! TS `AiProviderIcon`. Claude·Codex·Gemini·OpenCode는 `@lobehub/icons-static-svg` 그림을 `<img>`로 그리고,
//! 나머지 provider는 라벨 앞 두 글자를 대문자로 보인다.
//! 그림은 `<img>`라서 `currentColor`가 검정으로 풀린다(OpenCode 그림이 어두운 화면에서도 검정인 이유).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{AnyElement, Image, ImageFormat, IntoElement, ParentElement, Styled, div, img, px};
use kanvibe_ai::providers::AiSessionProvider;

use crate::components::TailwindText;

fn provider_svg(provider: AiSessionProvider) -> Option<&'static str> {
    match provider {
        AiSessionProvider::Claude => Some(include_str!("provider_icons/claude-color.svg")),
        AiSessionProvider::Codex => Some(include_str!("provider_icons/codex-color.svg")),
        AiSessionProvider::Gemini => Some(include_str!("provider_icons/gemini-color.svg")),
        AiSessionProvider::Opencode => Some(include_str!("provider_icons/opencode.svg")),
        _ => None,
    }
}

thread_local! {
    static PROVIDER_IMAGES: RefCell<HashMap<(AiSessionProvider, u32), Arc<Image>>> =
        RefCell::new(HashMap::new());
}

/// `size`px 정사각형 provider 아이콘
pub fn ai_provider_icon(provider: AiSessionProvider, size: f32) -> AnyElement {
    let Some(source) = provider_svg(provider) else {
        return div()
            .size(px(size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(10.0))
            .css_weight(700)
            .child(
                provider
                    .label()
                    .chars()
                    .take(2)
                    .collect::<String>()
                    .to_uppercase(),
            )
            .into_any_element();
    };
    let image = PROVIDER_IMAGES.with(|images| {
        images
            .borrow_mut()
            .entry((provider, size.to_bits()))
            .or_insert_with(|| {
                let sized = source
                    .replace(r#"height="1em""#, &format!(r#"height="{size}""#))
                    .replace(r#"width="1em""#, &format!(r#"width="{size}""#))
                    .replace("currentColor", "#000000");
                Arc::new(Image::from_bytes(ImageFormat::Svg, sized.into_bytes()))
            })
            .clone()
    });
    img(image).size(px(size)).flex_none().into_any_element()
}
