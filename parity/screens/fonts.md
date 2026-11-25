# 기준 이미지 글꼴 고정

기준 이미지는 `fonts.json`에 적힌 15개 파일만 보이는 fontconfig 설정(`FONTCONFIG_FILE`, 앱 프로세스 전용)으로 찍었다.
GPUI 앱은 아래 표의 파일을 그대로 번들하고, 같은 글꼴 선택 순서와 렌더링 옵션을 써야 픽셀이 맞는다.

## 앱의 글꼴 지정과 실제로 고른 파일

앱 CSS가 요청하는 글꼴은 이 머신에 없다. Chromium은 이름이 정확히 같은 글꼴만 받아들이므로 목록 끝의 일반 이름(generic family)까지 내려가고, 그 일반 이름을 fontconfig가 아래 파일로 풀었다.
"실제 사용"은 `capture.py --font-audit`(CDP `CSS.getPlatformFontsForNode`)로 130장 전체에서 모은 결과다.

| 앱이 요청한 글꼴(CSS) | 쓰이는 곳 | 결과 family | 파일 |
|---|---|---|---|
| `'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif` | 본문·UI 전체(`--font-sans`) | Noto Sans (Regular 400 / Bold 700) | `NotoSans-Regular.ttf`, `NotoSans-Bold.ttf` |
| 위와 같음, 한글 | 한글 글자 | Noto Sans CJK KR (ttc 안의 KR 얼굴) | `NotoSansCJK-Regular.ttc`, `NotoSansCJK-Bold.ttc` |
| `'JetBrains Mono', 'Fira Code', 'Consolas', monospace` | 코드·브랜치·단축키(`--font-mono`) | DejaVu Sans Mono | `DejaVuSansMono.ttf`, `DejaVuSansMono-Bold.ttf` |
| `'JetBrainsMono Nerd Font Mono', monospace` | xterm 터미널(14px). Nerd Font는 jsDelivr CDN에서 받는데 네트워크를 막아 실패한다 | DejaVu Sans Mono | 위와 같음 |
| Monaco 기본(`Droid Sans Mono`, `monospace`) | Diff·편집기(13px) | DejaVu Sans Mono | 위와 같음 |
| 기호·이모지 대체 | 위 글꼴에 없는 글자 | Noto Sans Symbols / Symbols2 / DejaVu Sans / Noto Color Emoji | 표 아래 목록 |

130장 전체 조사 결과(글리프 수, 쓰인 이미지 수): Noto Sans CJK KR Regular 41113자/122장, Noto Sans Regular 37604자/127장, DejaVu Sans Mono 17140자/63장, Noto Sans Bold 16721자/94장, DejaVu Sans Mono Bold 186자/7장(Monaco), Monaco 내장 `codicon` 웹 글꼴 25자/4장(`node_modules/monaco-editor`가 제공, 시스템 글꼴 아님), Noto Sans Symbols 10자/5장(`⌄`·`⌃` 등), Noto Sans Symbols2 2자/1장. DejaVu Sans, Noto Color Emoji, CJK Bold 파일은 쓰이지 않았다.

**한글 굵은 글씨는 합성 bold다.** 조사에서 CJK는 Regular 얼굴만 나왔다. 굵은 한글 제목(`font-semibold`·`font-bold`)은 Chromium이 Regular 얼굴을 합성 굵게(fake bold) 그린 것이다. GPUI도 같은 방식으로 그려야 픽셀이 맞는다.

font-weight 500·600은 파일이 없어 Chromium이 가까운 굵기(500→Regular, 600→Bold)를 쓴다. GPUI도 굵기 합성 없이 같은 파일을 골라야 한다.

## 파일 목록(fonts.json과 같다)

| 파일 | 패키지(Ubuntu) | 크기 | sha256 앞 16자 |
|---|---|---:|---|
| /usr/share/fonts/truetype/noto/NotoSans-Regular.ttf | fonts-noto-core 20201225-2build1 | 512672 | 89c3c497f618fdaa |
| /usr/share/fonts/truetype/noto/NotoSans-Bold.ttf | fonts-noto-core | 515752 | e83493c945848ecd |
| /usr/share/fonts/truetype/noto/NotoSans-Italic.ttf | fonts-noto-core | 530120 | 5d7618dda819443f |
| /usr/share/fonts/truetype/noto/NotoSans-BoldItalic.ttf | fonts-noto-core | 534180 | 4315b26e5afb62f7 |
| /usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf | fonts-dejavu-mono 2.37-8build1 | 343140 | a54dca07c76d6289 |
| /usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf | fonts-dejavu-mono | 334268 | 2d67eed9b325ee2e |
| /usr/share/fonts/truetype/dejavu/DejaVuSansMono-Oblique.ttf | fonts-dejavu-mono | 253448 | a033f17e7e429341 |
| /usr/share/fonts/truetype/dejavu/DejaVuSansMono-BoldOblique.ttf | fonts-dejavu-mono | 254960 | 3e19ce950e77f59d |
| /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf | fonts-dejavu-core 2.37-8build1 | 759720 | b4c632e3cdf9acc7 |
| /usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf | fonts-dejavu-core | 708920 | e1d733afbbfce842 |
| /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc | fonts-noto-cjk 1:20240730+repack1-1build1 | 19484784 | b76b0433203017ca |
| /usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc | fonts-noto-cjk | 20050760 | faa5f3656a78b2e2 |
| /usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf | fonts-noto-core | 258148 | 24b615ceb7d50a2c |
| /usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf | fonts-noto-core | 656852 | 876598db5f1e3bef |
| /usr/share/fonts/truetype/noto/NotoColorEmoji.ttf | fonts-noto-color-emoji 2.051-1build1 | 11020136 | 9fd0a3d0ce84d77e |

## 선택 순서와 렌더링 옵션

- 일반 이름 별칭: `sans-serif`·`sans`·`system-ui` → Noto Sans, Noto Sans CJK KR, Noto Sans Symbols, Noto Sans Symbols2, DejaVu Sans. `monospace`·`mono` → DejaVu Sans Mono, Noto Sans Mono CJK KR, Symbols, Symbols2. `serif`도 Noto Sans로 보낸다.
- 글자별 대체 순서(family 없이 charset·lang만 있는 질의): Noto Sans → Noto Sans CJK KR → Noto Sans Symbols → Noto Sans Symbols2 → DejaVu Sans → Noto Color Emoji. 이 규칙이 없으면 한글이 CJK JP 얼굴로 그려진다.
- fontconfig 2.17에서 `sans-serif` 질의는 `sans`로 정규화되므로 두 이름 모두 별칭을 둔다.
- 렌더링: antialias 켬, hinting 켬, hintstyle `hintslight`, rgba `none`(그레이스케일), lcdfilter 없음, autohint 끔, embedded bitmap 끔. Chromium에는 `--disable-lcd-text`, `--force-device-scale-factor=1`, `--force-color-profile=srgb`를 함께 준다.
- 크기: 본문 14px, `text-xs` 12px 등 CSS px 그대로(96dpi, 배율 1).
