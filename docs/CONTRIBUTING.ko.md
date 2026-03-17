# KanVibe 기여 가이드

[EN](../CONTRIBUTING.md) | [ZH](./CONTRIBUTING.zh.md)

KanVibe에 관심을 가져주셔서 감사합니다.

---

## 현재 아키텍처

KanVibe는 이제 데스크톱 전용 Tauri 앱으로 동작합니다.

- 활성 런타임: Tauri v2 + Rust backend
- 데스크톱 UI 호스트: `src-tauri/desktop/index.html`
- Rust 진입점: `src-tauri/src/main.rs`
- 로컬 영속화: `rusqlite` 기반 SQLite
- Next.js/Node 웹 런타임은 더 이상 활성 경로가 아님

---

## 개발 환경 설정

### 사전 요구사항

- Node.js 22+
- pnpm
- Rust toolchain (`rustup`, `cargo`)
- OS별 Tauri 실행에 필요한 GTK/WebKit 런타임 의존성
- 터미널/worktree 기능 검증용 tmux 또는 zellij

### 시작하기

```bash
# 저장소 클론
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# 환경 변수 복사
cp .env.example .env

# JS 측 도구 설치
pnpm install

# 데스크톱 앱 개발 모드 실행
pnpm dev
```

### 자주 쓰는 명령어

```bash
pnpm dev            # Tauri 데스크톱 앱 개발 모드 실행
pnpm check          # src-tauri 기준 cargo check
pnpm test           # src-tauri 기준 cargo test
pnpm lint           # cargo fmt --check
pnpm build          # 데스크톱 릴리스 바이너리 빌드
pnpm desktop:qa     # check + test + build 일괄 실행
```

---

## Tauri 개발 흐름

일반적인 작업 순서는 아래를 권장합니다.

1. `src-tauri/src/` 아래 Rust backend 코드 수정
2. 필요하면 `src-tauri/desktop/index.html` 데스크톱 UI 셸 수정
3. `pnpm check` 실행
4. `pnpm test` 실행
5. `pnpm dev`로 실제 데스크톱 앱 동작 확인

### 런타임 참고

- Tauri는 `src-tauri/tauri.conf.json`의 `frontendDist`에 지정된 로컬 데스크톱 자산을 로드합니다
- 활성 UI는 `window.__TAURI__.core.invoke(...)`로 Rust command를 호출합니다
- SQLite 스키마 초기화 로직은 현재 `src-tauri/src/backend/db.rs`에 있습니다

---

## 빌드 및 배포

### 로컬 릴리스 빌드

```bash
pnpm build
```

현재 생성되는 릴리스 산출물:

- 바이너리: `src-tauri/target/release/kanvibe-desktop`

### 실행 검증

빌드 후에는 실제로 바이너리가 실행되는지 확인하세요.

```bash
./src-tauri/target/release/kanvibe-desktop
```

헤드리스 Linux 환경에서는 Broadway 기반 smoke test를 사용할 수 있습니다.

```bash
broadwayd :7 >/tmp/kanvibe-broadway.log 2>&1 &
GDK_BACKEND=broadway BROADWAY_DISPLAY=:7 timeout 8s ./src-tauri/target/release/kanvibe-desktop
```

프로세스가 timeout 시점까지 살아 있으면 기동 검증 성공으로 간주해도 됩니다.

### 패키징 및 배포

현재 `src-tauri/tauri.conf.json`에서 `bundle.active`가 `false`라서 기본 릴리스 플로우는 실행 바이너리만 생성합니다.

설치 파일이나 OS 네이티브 번들을 만들고 싶다면:

1. `src-tauri/tauri.conf.json`에서 bundling 활성화
2. 플랫폼별 bundle target 및 signing 설정
3. 다시 `pnpm build` 실행

---

## Pull Request 가이드

### 제출 전 확인사항

1. 모든 체크가 통과해야 합니다:

```bash
pnpm check
pnpm test
pnpm build
```

2. 데스크톱 UI 동작이 바뀌었다면 스크린샷 또는 GIF를 첨부하세요.
3. 변경 사항은 데스크톱 전용 Tauri/Rust 아키텍처와 맞아야 합니다.

### PR 프로세스

1. 저장소를 Fork
2. `dev` 브랜치에서 작업 브랜치 생성
3. 변경 사항 작성
4. `pnpm desktop:qa` 실행
5. `pnpm dev` 또는 릴리스 바이너리 실행으로 동작 검증
6. Conventional Commits 형식으로 커밋
7. Push 후 Pull Request 생성

### 커밋 메시지 형식

```text
feat(scope): 새 기능 추가
fix(scope): 특정 버그 수정
docs: 문서 업데이트
refactor(scope): 코드 구조 개선
```

---

## 데이터베이스 변경

활성 런타임은 더 이상 TypeORM migration을 사용하지 않습니다.

영속화 동작을 변경할 때는:

1. `src-tauri/src/backend/db.rs`의 스키마 생성 로직 수정
2. 관련 Rust command의 읽기/쓰기 로직 수정
3. 기존 로컬 SQLite 데이터와의 호환성 확인

로컬 SQLite 기반 앱이므로, 파괴적인 스키마 변경은 명시적으로 계획된 경우에만 수행하세요.

---

## 문서 업데이트

사용자 또는 기여자 경험에 영향을 주는 변경을 하면 관련 문서도 함께 갱신하세요.

| 변경 내용 | 업데이트 대상 |
|-----------|--------------|
| 데스크톱 동작 또는 워크플로우 | `README.md`, `docs/README.ko.md`, `docs/README.zh.md` |
| 기여/릴리스 절차 | `CONTRIBUTING.md`, `docs/CONTRIBUTING.ko.md`, `docs/CONTRIBUTING.zh.md` |
| 환경 변수 | `.env.example` + 모든 README |
| Tauri 빌드/배포 절차 | README + 모든 CONTRIBUTING 문서 |

EN, KO, ZH 문서는 항상 함께 맞춰 주세요.

---

## 라이선스

KanVibe에 기여함으로써, 귀하의 기여는 [AGPL-3.0 라이선스](../LICENSE)에 따라 배포됨에 동의한 것으로 간주합니다.
