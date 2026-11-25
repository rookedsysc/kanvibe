# 화면 기준 이미지 캡처 도구

TS(Electron) 앱의 화면 상태 102개(이미지 130장)를 결정적으로 찍고, 같은 경로로 Rust(GPUI) 앱을 찍어 비교하기 위한 도구다.

## 구성

| 경로 | 역할 | TS 제거 후 |
|---|---|---|
| `parity/screens/states.yaml` | 상태별 선언: 창 크기, 경로, fixture, 서비스 고장, 단계(글자·역할 기준) | 유지 |
| `parity/screens/fonts.json`, `fonts.md` | 앱이 볼 수 있는 글꼴 파일과 렌더링 옵션 | 유지(GPUI가 같은 파일을 번들) |
| `parity/screens/baseline/` | 기준 이미지 `<ID>.png`, `<ID>--light.png`, `manifest.json`(실행별 결정성) | 유지 |
| `parity/fixtures/` | DB 스키마, 데이터셋, 로컬 git 저장소, 격리 HOME, 가짜 CLI, 네트워크 응답 | 유지 |
| `capture.py` | Xvfb 시작 → fixture 생성 → 드라이버 실행 → `ffmpeg -f x11grab` 캡처 → 결정성 비교 | 유지 |
| `diff.py` | PNG 픽셀 차이 비율. 기본은 pixelmatch YIQ 색 차이 허용치(0.1), `--strict`는 완전 일치. 두 비율을 함께 출력(Pillow, 없으면 순수 Python 디코더). 시험: `test_diff.py` | 유지 |
| `fontconfig.py` | 앱 프로세스 전용 `FONTCONFIG_FILE` 생성 | 유지 |
| `ts-driver/` | Electron + Playwright 드라이버와 main 프로세스 shim | **TS 제거 때 디렉터리째 삭제** |

## 실행

```bash
# 전체 130장을 두 번씩 찍고 baseline/과 manifest.json을 갱신한다(약 15~20분)
python3 parity/tools/screens/capture.py --runs 2 --keep-going

# 일부만
python3 parity/tools/screens/capture.py --only SCR-BOARD-04,SCR-DIFF-03 --runs 2

# 실제로 쓰인 글꼴 조사(이미지 옆에 <id>.fonts.json)
python3 parity/tools/screens/capture.py --only SCR-BOARD-04 --runs 1 --font-audit --out /tmp/audit

# 기준과 후보 디렉터리 비교(2% 기준)
python3 parity/tools/screens/diff.py --dirs parity/screens/baseline <candidate-dir> --threshold 0.02
```

중간 산출물(fixture, 실행별 이미지, 로그)은 `/tmp/kanvibe-parity/`에 생긴다. fixture 경로가 화면에 찍히므로 사용자 이름과 상관없는 이 고정 경로를 쓰며, `--root`를 바꾸면 기준 이미지와 달라진다. fixture 생성은 넘겨받은 경로만 쓰고 실제 `~/.claude`·`~/.codex`는 읽거나 쓰지 않는다.

필요한 것: `Xvfb`, `ffmpeg`(x11grab), `git`, `tmux`, Python 3 + PyYAML(+ Pillow 권장). TS 드라이버는 저장소의 `node_modules`(Playwright, Electron)와 `pnpm build` 산출물(`build/`)을 쓴다. 네이티브 모듈을 다시 빌드하지 않는다.

## 결정성 장치

- **창**: 창 관리자 없는 Xvfb, 화면 크기 = 창 크기(기본 1600x1000, 앱 기본 창 크기), 창은 (0,0). 캡처는 화면 전체를 `x11grab`으로 1프레임, 마우스 커서는 그리지 않는다.
- **데이터**: 매 캡처마다 `build_fixture.py`로 DB·git 저장소·HOME을 새로 만든다. 커밋 시각·작성자·UUID·타임스탬프가 고정이다.
- **시각**: 기준 시각 `2026-01-15T03:00:00Z`. 렌더러는 이 시각으로 고정하고, 백엔드는 이 시각에서 출발해 실제 경과 시간만큼만 흐른다.
- **네트워크**: 바깥 요청은 모두 막고 `parity/fixtures/network/<이름>.json`의 고정 응답만 돌려준다. Monaco는 CDN 대신 같은 버전의 로컬 `node_modules/monaco-editor`를 쓴다(TS 드라이버 한정). 터미널 Nerd Font CDN은 막혀 `monospace`로 떨어진다.
- **애니메이션**: 모든 CSS animation·transition과 텍스트 캐럿을 끄고 `prefers-reduced-motion: reduce`로 둔다.
- **글꼴**: `fonts.json`에 적힌 파일만 보이는 fontconfig, 그레이스케일 안티에일리어싱, hintslight.
- **환경 격리**: 앱 프로세스에만 격리 `HOME`, `XDG_*`, `TMUX_TMPDIR`, `FONTCONFIG_FILE`, `KANVIBE_APP_DATA_DIR`를 넘긴다. 사용자 셸·tmux 기본 소켓·전역 설정은 건드리지 않는다. 캡처가 끝나면 이 실행의 `TMUX_TMPDIR` tmux 서버만 끈다. 고정 hook 포트 47736(TLS 47746)을 쓴다.

## GPUI 앱용 드라이버가 지켜야 할 계약

`capture.py --driver-command "<명령>"`으로 다른 드라이버를 쓴다. 드라이버는 마지막 인자로 job JSON 경로를 받는다.

- job: `state`(states.yaml 항목, 기본값 병합), `env`(앱 프로세스에 그대로 넘길 환경), `window`, `fixedNowMs`, `hookPort`, `networkFixturePath`, `settleMs`
- 앱을 `env`로 띄우고 창을 (0,0)에 `window` 크기로 둔다. `state.faults`를 건 뒤 `state.route`를 열고 `state.steps`를 실행한다.
- 준비되면 stdout에 `{"event":"ready"}` 한 줄을 쓰고, stdin에서 `captured`를 받으면 앱을 닫고 `{"event":"done"}`을 쓴다. 실패는 `{"event":"error","message":...}`.
- 서비스 고장(`fault`)·보드 이벤트(`board_event`)·백엔드 알림(`main_send`)은 앱이 QA 모드에서 받을 수 있는 훅이 있어야 한다. 대상 이름은 TS의 IPC `namespace.method`를 식별자로 쓴다.
- 시각 고정(`fixedNowMs`)과 네트워크 응답 표(`networkFixturePath`)도 앱이 QA 모드에서 읽어야 한다.
