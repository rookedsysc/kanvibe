# KanVibe 기여 가이드

[EN](../CONTRIBUTING.md) | [ZH](./CONTRIBUTING.zh.md)

KanVibe에 관심을 가져주셔서 감사합니다! 이 가이드가 시작하는 데 도움이 될 것입니다.

---

## 개발 환경 설정

### 사전 요구사항

- Rust stable과 `rustfmt`, `clippy` (edition 2024)
- [cargo-nextest](https://nexte.st/)
- Herdr, tmux 또는 zellij (터미널 기능용)
- Linux에서만: GPUI와 WebKitGTK 개발 패키지. Ubuntu 예:
  `pkg-config libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libwebkit2gtk-4.1-dev`

### 시작하기

```bash
# 저장소 클론
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# 소스로 데스크톱 앱 실행 (디버그 빌드, 개발 DB)
cargo run -p kanvibe-app
```

디버그 빌드는 개발 데이터 디렉터리와 hook 포트 `19736`을 씁니다. [개발·QA 데이터베이스](./README.ko.md)를 참고하세요.

### 릴리스 빌드

```bash
# Linux: dist/에 tarball과 .deb (--verify는 풀어 낸 번들을 한 번 실행해 확인,
# --allow-missing-agents는 이 머신에서 빌드할 수 없는 원격 agent 대상을 건너뜀)
packaging/linux/build-linux.sh --verify

# macOS: 서명·공증한 KanVibe.app과 dist/KanVibe-${version}.dmg
packaging/macos/build-macos.sh
```

두 스크립트는 `kanvibe-app`을 `packaged` cargo 기능으로 빌드합니다. 이 바이너리만 설치된 앱의 데이터 디렉터리와 hook 포트 `9736`을 쓰고, 기능 없이 만든 `cargo build --release`는 수동 QA(`<appData>/kanvibe-qa`, 포트 `29736`)로 뜹니다.

버전은 루트 `Cargo.toml`의 `workspace.package.version`에서 읽습니다. `build-macos.sh`는 서명 자격 증명을 환경 변수나 `.env.example`에 적힌 키에서 읽고, `--unsigned`는 자격 증명 없이 로컬 확인용으로 빌드합니다.

- macOS 산출물: `dist/KanVibe-${version}.dmg`
- Homebrew cask: `Casks/kanvibe.rb` 갱신

---

## Pull Request 가이드

### 제출 전 확인사항

1. **모든 체크가 통과해야 합니다:**
   ```bash
   cargo fmt --all --check                                  # 포맷
   cargo clippy --workspace --all-targets -- -D warnings    # 린트
   cargo nextest run --workspace                            # 테스트
   cargo build --workspace --release                        # 릴리스 빌드
   ```

2. **동작하는 스크린샷 또는 GIF를 첨부하세요.** 기능 동작을 시각적으로 증명하지 않은 PR은 머지되지 않습니다.

3. **기존 코드 스타일을 따르세요.** 프로젝트에서 사용하는 규칙:
   - `crates/` 아래 Rust workspace (계층별 crate: `kanvibe-domain`, `kanvibe-db`, `kanvibe-services`, `kanvibe-app`, ...)
   - 의미 기반 테마 토큰(`crates/kanvibe-app/src/theme.rs`)을 쓰는 GPUI 뷰
   - 모든 사용자 텍스트는 `messages/*.json`
   - `rusqlite`로 접근하는 내장 SQLite (`kanvibe-db`)

### PR 프로세스

1. 저장소를 Fork
2. `main`에서 feature 브랜치 생성
3. 변경 사항 작성
4. 모든 체크 실행 (위 참고)
5. [Conventional Commits](https://www.conventionalcommits.org/) 형식으로 커밋
6. Push 후 Pull Request 생성
7. 동작하는 기능의 스크린샷/GIF 첨부

### 커밋 메시지 형식

```
feat(scope): 새 기능 추가
fix(scope): 특정 버그 수정
docs: 문서 업데이트
refactor(scope): 코드 구조 개선
```

---

## 국제화 (i18n)

모든 사용자 대면 문자열은 번역이 필요합니다. UI 텍스트를 추가하거나 수정할 때:

1. `messages/ko.json`에 키/값 추가
2. `messages/en.json`과 `messages/zh.json`에 번역 추가
3. 뷰에서는 `kanvibe_domain::i18n`(`Translator::translate` 또는 `translate`)으로 읽기

---

## 데이터베이스 변경

KanVibe는 내장 SQLite 데이터베이스(`crates/kanvibe-db`)를 사용합니다. 테이블 구조를 바꿀 때는:

1. `crates/kanvibe-db/src/migrations/mod.rs`의 `MIGRATIONS`에 마이그레이션 추가
2. 옛 DB도 같은 스키마에 이르도록 `crates/kanvibe-db/src/schema/`의 멱등 스키마 복구 갱신

---

## 문서 업데이트

사용자 대면 동작에 영향을 주는 변경을 할 때는 해당 문서도 함께 업데이트하세요:

| 변경 내용 | 업데이트 대상 |
|-----------|--------------|
| 기능 또는 UI | `README.md`, `docs/README.ko.md`, `docs/README.zh.md` |
| 기여 프로세스 | `docs/CONTRIBUTING.md`, `docs/CONTRIBUTING.ko.md`, `docs/CONTRIBUTING.zh.md` |
| 환경변수 | `.env.example` + 모든 README 파일 |
| Hook API | 모든 README 파일 (Hook API 섹션) |

세 가지 언어 버전 (EN, KO, ZH)은 항상 함께 업데이트해야 합니다.

---

## 다음 단계 & 로드맵

### Gemini Hooks / Codex Hooks 지원 (개발 중)

현재 KanVibe는 자동 상태 추적을 위해 **Claude Code Hooks**를 지원합니다. **Gemini Hooks**와 **Codex Hooks** 지원은 계획 중이며 개발 진행 중입니다.

멀티 에이전트 hook 지원에 대해 더 좋은 방향이나 아키텍처 아이디어가 있다면, 먼저 [Discussion](https://github.com/rookedsysc/kanvibe/discussions)을 열어주세요. 구현 전에 함께 최적의 방향을 찾고 싶습니다.

### 토큰 사용량 대시보드

다음 주요 기능 목표는 **토큰 사용량 추적 대시보드**입니다 - 태스크와 세션별 AI 에이전트 토큰 소비량을 모니터링합니다.

### 새 기능 제안 방법

중요한 변경이나 새 기능의 경우:
1. [Discussion](https://github.com/rookedsysc/kanvibe/discussions)에서 아이디어를 공유
2. 커뮤니티와 메인테이너의 피드백을 받기
3. 방향이 합의되면 Issue를 생성하고 PR 제출

---

## 라이센스

KanVibe에 기여함으로써, 귀하의 기여가 [AGPL-3.0 라이센스](../LICENSE)에 따라 라이선스됨에 동의합니다.
