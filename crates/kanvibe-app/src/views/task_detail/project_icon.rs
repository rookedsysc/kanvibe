//! 프로젝트 GitHub 아이콘(`icon_data_url`)을 그림으로 바꾼다. 같은 data URL은 한 번만 해석한다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{Image, ImageFormat};
use kanvibe_services::remote_image_paste::decode_data_url_to_bytes;

thread_local! {
    static DECODED_ICONS: RefCell<HashMap<String, Option<Arc<Image>>>> = RefCell::new(HashMap::new());
}

/// `data:image/<형식>;base64,...`. 해석할 수 없으면 `None`(TS `<img>`가 깨진 그림을 그리는 자리)
pub fn data_url_image(data_url: &str) -> Option<Arc<Image>> {
    DECODED_ICONS.with(|icons| {
        if let Some(image) = icons.borrow().get(data_url) {
            return image.clone();
        }
        let image = decode(data_url);
        icons
            .borrow_mut()
            .insert(data_url.to_string(), image.clone());
        image
    })
}

fn decode(data_url: &str) -> Option<Arc<Image>> {
    let mime = data_url.strip_prefix("data:")?.split([';', ',']).next()?;
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/webp" => ImageFormat::Webp,
        "image/gif" => ImageFormat::Gif,
        "image/svg+xml" => ImageFormat::Svg,
        _ => return None,
    };
    let bytes = decode_data_url_to_bytes(data_url).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_data_url_reuses_the_decoded_image() {
        let first = data_url_image("data:image/png;base64,aGk=").expect("png 아이콘");
        let second = data_url_image("data:image/png;base64,aGk=").expect("png 아이콘");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.bytes(), b"hi");
    }

    #[test]
    fn data_urls_that_are_not_images_have_no_icon() {
        assert!(data_url_image("data:text/plain,hi").is_none());
    }
}
