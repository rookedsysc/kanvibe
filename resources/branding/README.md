# KanVibe 브랜드 자산

`kanvibe-logo.png`는 제공받은 가로형 원본, `kanvibe-emblem.png`는 정사각형 엠블럼 원본이다. 가로형은 앱 내 제품명 표시에, 엠블럼은 런처·Dock·favicon에 쓴다.

저장소 루트에서 다음 명령으로 모든 크기를 다시 생성한다. uv가 스크립트에 고정된 Pillow를 격리 설치한다.

```sh
uv run scripts/generate-brand-assets.py
```

| 소비 위치 | 생성 파일 |
| --- | --- |
| 데스크톱 보드 헤더 | `public/branding/kanvibe-logo.png` (투명, 1024px 너비) |
| 원본 앱 아이콘 | `public/branding/app-icon.png` (1024px, 투명 여백과 흰색 타일) |
| Tauri 창·개발 실행 Dock | `src-tauri/icons/icon.png` (1024px, `public/branding/app-icon.png`와 동일) |
| macOS 앱·DMG | `src-tauri/icons/icon.icns` (32–1024px, 16pt@2x 포함) |
| 렌더러 favicon | `public/favicon.png` (64px) |
| README 라이트·다크 로고 | `public/branding/kanvibe-logo{,-light}.png` (alpha 보존) |
| 문서 사이트 헤더 | `docs-site/public/branding/kanvibe-logo.png` |
| 문서 사이트 아이콘 | `docs-site/public/favicon.ico` (16–256px), `icon.png` (512px), `apple-icon.png` (180px) |
| 문서 사이트 공유 이미지 | `docs-site/public/branding/og-image.png` (1200×630px, 고정 사이트 URL 설정 시 metadata 연결) |
| 모바일 연결 화면 | `mobile/assets/branding/kanvibe-logo.png` |
| iPhone·iPad·App Store | `mobile/ios/Runner/Assets.xcassets/AppIcon.appiconset/` (Contents.json의 크기, 불투명 RGB) |
| Android 기존 런처 | `mobile/android/app/src/main/res/mipmap-*/ic_launcher.png` (48/72/96/144/192px) |
| Android 적응형·테마 아이콘 | `mobile/android/app/src/main/res/drawable-*/ic_launcher_foreground.png` (108/162/216/324/432px) |

가로형은 비율을 유지하고 투명 여백만 제거한다. 다크 테마에서는 데스크톱 CSS 반전과 모바일 `AppColors.textPrimary` tint로 대비를 유지한다. iOS는 흰색 정사각형 원본을 OS가 마스킹한다.

Android 전경은 108dp 레이어 중앙의 48dp 영역에 들어가며, 흰 배경과 분리되어 있다. [Android 적응형 아이콘 지침](https://developer.android.com/develop/ui/compose/system/icon_design_adaptive)의 “safe zone of 66x66 dp” 안에 K 모양이 들어가도록 했다. Android 13부터는 동일 전경의 alpha를 monochrome 레이어로 쓴다. 데스크톱 창과 번들 아이콘은 `src-tauri/tauri.conf.json`의 `bundle.icon`이 가리키는 `src-tauri/icons/`를 쓴다.
