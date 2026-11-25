# 화면 기준 이미지 비교 도구

TS(Electron) 앱의 화면 상태 102개(이미지 130장)를 결정적으로 찍어 둔 기준 이미지와, 같은 방법으로 찍은 Rust(GPUI) 앱 화면을 비교하기 위한 도구다. 기준 이미지를 찍던 TS 드라이버와 그 실행기는 TS 코드와 함께 지웠다(커밋 `a73f3498`까지 git 기록에 있다). 기준 이미지는 고정된 데이터다.

## 구성

| 경로 | 역할 |
|---|---|
| `parity/screens/states.yaml` | 상태별 선언: 창 크기, 경로, fixture, 서비스 고장, 단계(글자·역할 기준) |
| `parity/screens/fonts.json`, `fonts.md` | 앱이 볼 수 있는 글꼴 파일과 렌더링 옵션(GPUI가 같은 파일을 번들) |
| `parity/screens/baseline/` | 기준 이미지 `<ID>.png`, `<ID>--light.png`, `manifest.json`(실행별 결정성) |
| `parity/fixtures/` | DB 스키마, 데이터셋, 로컬 git 저장소, 격리 HOME, 가짜 CLI, 네트워크 응답 |
| `capture.py` | 공용 부품: states.yaml 읽기, fixture 생성, Xvfb 시작, 격리 앱 환경, `ffmpeg -f x11grab` 캡처 |
| `diff.py` | PNG 픽셀 차이 비율. 기본은 pixelmatch YIQ 색 차이 허용치(0.1), `--strict`는 완전 일치. 두 비율을 함께 출력(Pillow, 없으면 순수 Python 디코더). 시험: `test_diff.py` |
| `fontconfig.py` | 앱 프로세스 전용 `FONTCONFIG_FILE` 생성 |
| `../screens-rust/capture_rust.py` | GPUI 앱을 `kanvibe --qa-harness`로 띄워 상태를 재현하고 찍은 뒤 기준 이미지와 비교 |

## 실행

```bash
# Rust 앱 빌드 뒤 일부 상태를 찍고 기준 이미지와 비교한다
cargo build -p kanvibe-app
python3 parity/tools/screens-rust/capture_rust.py --only SCR-NF-01,SCR-BOARD-05 --diff

# 기준과 후보 디렉터리 비교(2% 기준)
python3 parity/tools/screens/diff.py --dirs parity/screens/baseline <candidate-dir> --threshold 0.02
```

중간 산출물(fixture, 이미지, 로그)은 `/tmp/kanvibe-parity-rust/`에 생긴다. fixture 생성은 넘겨받은 경로만 쓰고 실제 `~/.claude`·`~/.codex`는 읽거나 쓰지 않는다.

필요한 것: `Xvfb`, `ffmpeg`(x11grab), `git`, `tmux`, Python 3 + PyYAML(+ Pillow 권장).

## 결정성 장치

- **창**: 창 관리자 없는 Xvfb, 화면 크기 = 창 크기(기본 1600x1000, 앱 기본 창 크기), 창은 (0,0). 캡처는 화면 전체를 `x11grab`으로 1프레임, 마우스 커서는 그리지 않는다.
- **데이터**: 매 캡처마다 `build_fixture.py`로 DB·git 저장소·HOME을 새로 만든다. 커밋 시각·작성자·UUID·타임스탬프가 고정이다.
- **시각**: 기준 시각 `2026-01-15T03:00:00Z`. 앱은 `--fixed-now`로 이 시각에 고정한다.
- **네트워크**: 바깥 요청은 모두 막고 `parity/fixtures/network/<이름>.json`의 고정 응답만 돌려준다(`--network-responses`).
- **글꼴**: `fonts.json`에 적힌 파일만 보이는 fontconfig, 그레이스케일 안티에일리어싱, hintslight.
- **환경 격리**: 앱 프로세스에만 격리 `HOME`, `XDG_*`, `TMUX_TMPDIR`, `FONTCONFIG_FILE`, `KANVIBE_APP_DATA_DIR`를 넘긴다. 사용자 셸·tmux 기본 소켓·전역 설정은 건드리지 않는다. 캡처가 끝나면 이 실행의 `TMUX_TMPDIR` tmux 서버만 끈다.

기준 이미지는 TS 쪽에서 CSS animation·transition과 텍스트 캐럿을 끄고 `prefers-reduced-motion: reduce`로 찍었다. Monaco는 같은 버전의 로컬 번들을 썼고 터미널 Nerd Font CDN은 막혀 `monospace`로 떨어졌다.
