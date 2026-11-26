//! `mobileDiagramRenderService.test.ts`의 입력·이미지 경계 시험.

use base64::Engine;
use kanvibe_diagram::contract::{
    IMAGE_BYTES_LIMIT, MobileDiagramError, parse_mobile_diagram_input,
    validate_mobile_diagram_image,
};
use serde_json::{Value, json};

const VALID_TINY_JPEG: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAIBAQEBAQIBAQECAgICAgQDAgICAgUEBAMEBgUGBgYFBgYGBwkIBgcJBwYGCAsICQoKCgoKBggLDAsKDAkKCgr/2wBDAQICAgICAgUDAwUKBwYHCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgr/wAARCAACAAIDAREAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD9YP8AgnB8CPgf4l/4J4/AXxF4i+DXhTUNQ1D4L+Frm/v73w7bSzXMz6Tas8kjshZ3ZiWLEkkkk16Hij4e8A4XxMzuhQynDQhDGYlRiqFJJJVppJJQskloktEj4/EcF8HZ/XnmeZ5bh6+Jrt1KtWpRpzqVKk3zTqVJyi5TnOTcpzk3KUm222z/2Q==";

fn tiny_jpeg() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(VALID_TINY_JPEG)
        .expect("jpeg")
}

fn find(image: &[u8], marker: [u8; 2]) -> usize {
    image
        .windows(2)
        .position(|window| window == marker)
        .expect("marker")
}

fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut image = tiny_jpeg();
    let marker = find(&image, [0xff, 0xc0]);
    image[marker + 5..marker + 7].copy_from_slice(&height.to_be_bytes());
    image[marker + 7..marker + 9].copy_from_slice(&width.to_be_bytes());
    image
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; 33];
    bytes[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    bytes[12..16].copy_from_slice(b"IHDR");
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    bytes
}

fn input() -> Value {
    json!({ "definition": "flowchart LR\nA[Start]-->B[Done]", "theme": "light", "width": 640 })
}

fn with(field: &str, value: Value) -> Value {
    let mut request = input();
    request[field] = value;
    request
}

#[test]
fn rejects_four_component_scans_even_when_segment_bounds_are_consistent() {
    let image = jpeg(640, 120);
    let scan = find(&image, [0xff, 0xda]);
    let old_length = usize::from(u16::from_be_bytes([image[scan + 2], image[scan + 3]]));
    let four_components = [0, 14, 4, 1, 0, 2, 0, 3, 0, 4, 0, 0, 63, 0];
    let invalid = [
        &image[..scan + 2],
        &four_components[..],
        &image[scan + 2 + old_length..],
    ]
    .concat();
    assert!(validate_mobile_diagram_image(&invalid, 640).is_err());
}

#[test]
fn recognizes_a_real_jpeg_container_and_preserves_mime_plus_sof_dimensions() {
    let tiny = validate_mobile_diagram_image(&tiny_jpeg(), 2).expect("tiny");
    assert_eq!(
        (tiny.width, tiny.height, tiny.mime_type),
        (2, 2, "image/jpeg")
    );
    let large = validate_mobile_diagram_image(&jpeg(1200, 2400), 1200).expect("large");
    assert_eq!(
        (large.width, large.height, large.mime_type),
        (1200, 2400, "image/jpeg")
    );
    assert_eq!(
        validate_mobile_diagram_image(&png(640, 120), 640)
            .expect("png")
            .mime_type,
        "image/png"
    );
}

#[test]
fn rejects_jpeg_size_mismatches_missing_eoi_malformed_lengths_and_unsupported_frames() {
    assert!(validate_mobile_diagram_image(&jpeg(639, 120), 640).is_err());
    assert!(validate_mobile_diagram_image(&jpeg(640, 2401), 640).is_err());
    let truncated = jpeg(640, 120);
    assert!(validate_mobile_diagram_image(&truncated[..truncated.len() - 2], 640).is_err());
    let mut bad_length = jpeg(640, 120);
    bad_length[4..6].copy_from_slice(&1u16.to_be_bytes());
    assert!(validate_mobile_diagram_image(&bad_length, 640).is_err());
    let mut overflow_length = jpeg(640, 120);
    overflow_length[4..6].copy_from_slice(&65535u16.to_be_bytes());
    assert!(validate_mobile_diagram_image(&overflow_length, 640).is_err());
    let mut lossless = jpeg(640, 120);
    let frame = find(&lossless, [0xff, 0xc0]);
    lossless[frame + 1] = 0xc3;
    assert!(validate_mobile_diagram_image(&lossless, 640).is_err());
    let mut precision = jpeg(640, 120);
    let frame = find(&precision, [0xff, 0xc0]);
    precision[frame + 4] = 12;
    assert!(validate_mobile_diagram_image(&precision, 640).is_err());
    let oversized = [&[0xff, 0xd8][..], &vec![0u8; IMAGE_BYTES_LIMIT]].concat();
    assert!(validate_mobile_diagram_image(&oversized, 640).is_err());
}

#[test]
fn does_not_accept_a_dimensions_only_fake_jpeg_without_a_scan() {
    let image = jpeg(640, 120);
    let scan = find(&image, [0xff, 0xda]);
    let fake = [&image[..scan], &[0xff, 0xd9][..]].concat();
    assert!(validate_mobile_diagram_image(&fake, 640).is_err());
}

#[test]
fn rejects_unsafe_or_invalid_input() {
    let cases = [
        Value::Null,
        json!([]),
        json!({}),
        with("width", json!(319)),
        with("width", json!(1201)),
        with("width", json!(640.5)),
        with("theme", json!("neutral")),
        with("theme", json!(["light"])),
        with("theme", json!({ "value": "dark" })),
        with("definition", json!(" ")),
        with("definition", json!("가".repeat(21846))),
        with(
            "definition",
            json!("%%{init: {securityLevel: 'loose'}}%%\nflowchart LR\nA-->B"),
        ),
        with(
            "definition",
            json!("---\nconfig: {}\n---\nflowchart LR\nA-->B"),
        ),
    ];
    for case in cases {
        let error: MobileDiagramError =
            parse_mobile_diagram_input(&case).expect_err("거절해야 합니다");
        assert_eq!(error.status_code, 400, "{case}");
    }
}

#[test]
fn accepts_exact_utf8_source_and_width_boundaries() {
    let longest = with("definition", json!("a".repeat(65536)));
    let mut longest = longest;
    longest["width"] = json!(320);
    assert_eq!(
        parse_mobile_diagram_input(&longest)
            .expect("longest")
            .definition
            .len(),
        65536
    );
    let mut widest = with("theme", json!("dark"));
    widest["width"] = json!(1200);
    assert_eq!(
        parse_mobile_diagram_input(&widest).expect("widest").width,
        1200
    );
}

#[test]
fn reads_actual_png_dimensions_and_rejects_mismatches_magic_height_and_bytes() {
    let image = validate_mobile_diagram_image(&png(640, 120), 640).expect("png");
    assert_eq!((image.width, image.height), (640, 120));
    assert!(validate_mobile_diagram_image(&png(639, 120), 640).is_err());
    assert!(validate_mobile_diagram_image(&png(640, 2401), 640).is_err());
    assert!(validate_mobile_diagram_image(&[0u8; 33], 640).is_err());
    let oversized = [png(640, 120), vec![0u8; IMAGE_BYTES_LIMIT]].concat();
    assert!(validate_mobile_diagram_image(&oversized, 640).is_err());
}

#[test]
fn error_messages_match_the_ts_contract_goldens() {
    let bad_theme = json!({ "definition": "graph TD; A-->B", "theme": "blue", "width": 400 });
    assert_eq!(
        parse_mobile_diagram_input(&bad_theme)
            .expect_err("theme")
            .message,
        "잘못된 다이어그램 요청입니다"
    );
    let directive =
        json!({ "definition": "%%{init: {}}%%\ngraph TD; A-->B", "theme": "dark", "width": 400 });
    let error = parse_mobile_diagram_input(&directive).expect_err("directive");
    assert_eq!(
        (error.status_code, error.message.as_str()),
        (400, "다이어그램 설정 지시문은 지원하지 않습니다")
    );
}
