//! 배포 패키지 설정(TS `electron-builder.yml`의 productName·icon)을 Rust 패키징 파일(`packaging/`)에서 확인한다.

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative_path: &str) -> String {
    std::fs::read_to_string(repository_root().join(relative_path))
        .unwrap_or_else(|error| panic!("{relative_path}: {error}"))
}

/// `<key>{key}</key>` 바로 다음 `<string>` 값
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after_key = plist.split_once(&format!("<key>{key}</key>"))?.1;
    let value = after_key.trim_start().strip_prefix("<string>")?;
    value
        .split_once("</string>")
        .map(|(value, _)| value.to_string())
}

// parity: TS-lib-packageScripts-77e497
#[test]
fn packaged_apps_are_named_kanvibe() {
    let plist = read("packaging/macos/Info.plist.in");
    let desktop_entry = read("packaging/linux/kanvibe.desktop");

    assert_eq!(
        plist_string(&plist, "CFBundleName").as_deref(),
        Some("KanVibe")
    );
    assert_eq!(
        plist_string(&plist, "CFBundleDisplayName").as_deref(),
        Some("KanVibe")
    );
    assert!(desktop_entry.lines().any(|line| line == "Name=KanVibe"));
    assert!(!plist.contains("Kanivibe") && !desktop_entry.contains("Kanivibe"));
}

// parity: TS-lib-electronBuilderConfig-1c3bdc
#[test]
fn the_mac_app_uses_the_kanvibe_icon_from_resources() {
    let plist = read("packaging/macos/Info.plist.in");
    let build_script = read("packaging/macos/build-macos.sh");

    assert_eq!(
        plist_string(&plist, "CFBundleIconFile").as_deref(),
        Some("icon.icns")
    );
    assert!(
        build_script.contains(
            r#"install -m 0644 "$repo_root/resources/icon.icns" "$app/Contents/Resources/icon.icns""#
        ),
        "build-macos.sh가 resources/icon.icns를 앱 번들에 넣어야 한다"
    );
    assert!(repository_root().join("resources/icon.icns").is_file());
}
