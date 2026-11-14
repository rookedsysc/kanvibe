---
title: KanVibe에서 기존 Herdr 작업을 연결하고 이전하는 계획
reader: 독자: 맥북과 원격 PC에서 작업하며 기존 세션의 작업을 잃지 않고 정리하려는 개발자
profile: prd
---

<summary-box>
기존 작업을 그대로 쓰려면 원래 Herdr 세션에 연결한다.
세션을 하나로 합치려면 중단된 작업의 구조를 재구성해야 한다.
실행 중인 프로세스의 세션 간 이동은 이 계획에서 보장하지 않는다.

- 연결 방식은 기존 서버와 프로세스를 유지한다.
- 재구성 방식은 같은 작업 경로에 새 Space를 만든다.
- 설정의 Herdr 세션 관리 화면에 연결과 재구성을 구현했다.
- 아래 구현 현황이 실제 지원 범위다. 설계 재생기의 예시는 실행 결과가 아니다.
</summary-box>

## 구현 현황: 지금 사용할 수 있는 기능 {#implementation-current toc="구현 현황"}

2026-11-13 기준으로 설정 → Herdr 세션 관리에서 시작한다.
로컬 또는 SSH 호스트를 선택한 뒤 목록 조회를 누른다.
세션과 Space를 고르고 기존 태스크에 연결하거나 **새 태스크로 가져오기**를 선택한다.
계획에서 보존·재구성·미지원 항목을 확인한 뒤 직접 실행한다.
조회와 미리보기는 원본 서버를 시작하거나 태스크를 만들지 않는다.

| 선택 | 실제 처리 | 보존과 제한 |
| --- | --- | --- |
| 기존 Space에 연결 | 실행 중인 원본 서버의 Space를 태스크에 연결한다. 원격이면 로컬 Herdr machine 프로필도 등록한다 | 원본 프로세스를 유지한다. 비활성 프로필을 자동 활성화하지 않는다 |
| 중단된 Space 재구성 | Herdr v0.9.5의 version 3 snapshot을 백업하고 같은 호스트의 kanvibe에 새 셸과 배치를 만든다 | 원본은 남는다. 실행 중인 원본과 지원하지 않는 구조는 차단한다 |
| 새 태스크로 가져오기 | B에 태스크가 없어도 선택한 원격 Space를 가져온다 | 실행 검증이 끝난 뒤 태스크와 작업 기록을 같은 DB 트랜잭션으로 저장한다 |
| 진행 확인과 취소 | 작업 ID로 진행을 조회하고 단계 사이에서 취소한다 | 생성된 Space는 삭제하지 않는다. 결과가 불명확하면 attention으로 남긴다 |
| 이전 연결 복원 | 다른 창이 연결을 바꾸지 않았고 이전 서버가 실행 중일 때 태스크 연결만 되돌린다 | 파일·프로세스·Space는 되돌리거나 삭제하지 않는다. 새로 가져온 태스크에는 이전 연결이 없다 |

KanVibe는 태스크별 기존 소켓 프록시로 선택한 Space를 연다.
Herdr 0.9.5에는 외부 요청으로 특정 로컬 TUI 창의 machine과 Space를 함께 선택하는 공개 API가 없다.
따라서 전역 endpoint 선택 파일을 바꾸거나 키 입력을 주입하지 않는다.
외부 Herdr에서는 등록된 machine을 사용자가 선택한다.

재구성은 탭·분할·비율·경로·이름·선택·확대 상태를 읽어서 새 대상과 대조한다.
분할 비율 0.1~0.9 밖의 값, 첫 pane과 다른 내부 root, 축약 홈 경로처럼 공개 CLI로 정확히 복원할 수 없는 값은 계획에서 차단한다.
원본 snapshot의 launch 명령이나 에이전트 재개 명령은 실행하지 않는다.
**별도의 에이전트 대화 재개 버튼은 이번 구현에 포함하지 않았다.**
실행 중인 대화는 기존 Space 연결로 계속 사용한다.
중단된 대화의 참조는 원본과 백업에 남으며, 새 셸에서 제공자 CLI로 직접 재개해야 한다.

작업 기록은 SQLite의 herdr_session_operations에 저장한다.
태스크 연결에는 기존 Space용 herdr:서버:Space 또는 새 공유 Space용 kanvibe:태스크ID를 저장한다.
kanvibe-workspaces.json은 새 공유 Space와 태스크를 연결한다.
원본 백업은 원본 session 디렉터리의 kanvibe-backups/작업ID.json에 권한 0600으로 저장한다.
앱 재시작 뒤 applying·verifying·committed 기록은 자동 재실행하지 않고 attention으로 표시한다.
원격 생성과 SQLite 저장 사이에 완전한 원자성은 없다.
KanVibe의 잠금은 외부 Herdr 실행까지 막지 못한다.

구현 검증에서는 실제 Herdr 격리 세션의 2개 탭·5개 pane 복원과 읽기 검증을 통과했다.
단위·회귀 검사 383개, 전체 타입 검사, 변경 파일 린트, renderer/main 빌드가 통과했다.
실제 빌드된 Electron과 Herdr의 격리 통합 검사 5개도 통과했다.
태스크 0개에서 기존 Space를 가져온 뒤 pane 안의 프로세스 PID가 유지됐다.
중단된 snapshot의 탭 2개·pane 3개를 복원했고 원본·백업 바이트 동일과 연결 변경을 확인했다.
이전 서버가 중단된 경우 복원을 차단하며 현재 연결과 두 Space를 보존했다.
해당 검사는 IPC mock 없이 수행했고 renderer 예외는 0개였다.
사용자의 실제 세션은 자동 이전하지 않았다.
macOS B ↔ 실제 원격 Linux A의 종단간 입력과 클립보드는 이 환경에서 검증하지 않았다.

이후의 proposed 절과 재생기는 최초 설계 기록이다.
API 이름과 자동 대화 재개 등 차이가 있으면 이 구현 현황을 따른다.

## Test Case Decision {#test-case-decision toc="Test Case Decision"}

다음은 구현을 시작할 때 적용할 검증 제안이다.
이 문서 제작과 브라우저 검사는 기능 구현 승인이나 제품 QA 완료를 뜻하지 않는다.

| 검증 | 입력과 기대 결과 | 결정 |
| --- | --- | --- |
| 기존 실행 보존 | 실행 중인 원본에 연결하면 원본 서버와 pane의 PID가 유지된다 | Required 제안 |
| 이동 제한 | running 원본으로 재구성을 요청하면 쓰기 전에 차단한다 | Required 제안 |
| 재구성 | stopped이며 지원하는 저장 형식이면 새 Space와 연결 정보를 검증한다 | Required 제안 |
| 재시도와 경쟁 | 같은 operationId를 재요청하면 같은 작업을 조회하고 중복 Space를 만들지 않는다 | Required 제안 |
| 실패와 재시작 | 원격 생성 뒤 앱이 종료되면 새 실행에서 미완료 작업을 판별한다 | Required 제안 |
| 실제 사용 | macOS 클라이언트와 Linux 원격에서 입력, 창 독립성, 복구를 검증한다 | Required 제안 |

| PRD 프로파일 | 이유 | 플랫폼 | 문서 상태 | 코드 상태 |
| --- | --- | --- | --- | --- |
| full | 원격 연결, 기존 세션 재구성, 화면과 저장 계약을 함께 설명한다 | backend, web | draft | proposed |

현재 사실은 current, 공식 동작과 일치하는 검증은 verified로 표시한다.
제안은 proposed, 미해결 판단은 open으로 표시한다.
proposed 절은 최초 설계 기록이다.
현재 제품 지원 범위는 [구현 현황](#implementation-current)을 따른다.

| 내용 묶음 | 적용 | 원문 | 생략 이유 |
| --- | --- | --- | --- |
| requirements | yes | [원격 연결 목표](#REQ-connect) | - |
| policy | yes | [보존 규칙](#RULE-preserve) | - |
| ux | yes | [검사 화면](#SCR-inventory) | - |
| api | yes | [검사 계약](#EVT-scan) | - |
| technical | yes | [책임 경계](#TECH-boundary) | - |
| validation | yes | [검증 계획](#QA-transition) | - |
| release | yes | [출시 순서](#REL-rollout) | - |

## 1. 요구사항: 무엇을 같은 작업으로 볼 것인가 {#requirements toc="1. 요구사항"}

### 먼저 구분할 세 가지 목표 {#goals toc="먼저 구분할 세 가지 목표"}

A는 기존 작업이 실행되는 PC다.
B는 그 작업을 보는 맥북이다.
Herdr 세션은 각 서버가 소유하는 작업 단위다.
Space는 한 Herdr 세션 안의 작업 공간이다.
에이전트 세션은 Codex나 Claude의 대화 기록이다.
PID는 운영체제가 프로세스에 붙인 식별자다.
세 이름의 세션은 서로 바꾸어 쓰지 않는다.

| 질문 | 연결해서 보기 | 새 Space로 재구성 | 프로세스의 실제 이동 |
| --- | --- | --- | --- |
| 무엇이 달라지는가 | B의 연결 대상과 화면 | 같은 호스트의 새 서버 안에 작업 구조 생성 | 프로세스와 터미널의 소유 서버 |
| 작업이 실행되는 곳 | 원래 A | 원래 A 또는 원래 B | 이동 기능에 따라 다름 |
| 기존 프로세스 | 원래 서버에서 유지 | 이어받지 않음 | 현재 지원 기능으로 보장 불가 |
| Herdr session list | 기존 목록 유지 | 원본을 남기면 목록 유지 | 병합 명령 확인 안 됨 |
| 이 계획의 위치 | 첫 번째 구현 대상 | 두 번째 구현 대상 | 구현 범위 제외 |

현재 기능과 제안의 차이를 구분해야 한다.
Herdr의 machine 등록은 한 프로필에 원격 세션 하나를 연결한다.
프로필 등록은 여러 서버를 한 서버로 합치지 않는다.
[공식 연결 계약](https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/connecting-machines.mdx)을 기준으로 한다.

### 무엇을 보장하고 무엇을 보장하지 않는가 {#guarantees toc="무엇을 보장하고 무엇을 보장하지 않는가"}

아래의 보장은 조건을 함께 읽어야 한다.
구현 제안의 보장은 제품 검증을 통과한 뒤에만 제공한다.

| 보존 대상 | 기존 세션에 연결 | stopped 원본 재구성 | 보장 경계 |
| --- | --- | --- | --- |
| 수정한 파일과 미커밋 변경 | 같은 디렉터리의 파일 사용 | 같은 디렉터리의 파일 사용 | 파일 복사나 Git reset을 실행하지 않는 제안이다 |
| 작업 경로 | 원본 경로 사용 | 존재와 접근 권한 확인 후 사용 | 경로가 없으면 다른 경로로 조용히 대체하지 않는다 |
| 탭과 분할 배치 | 실행 중인 원본 화면 사용 | 지원하는 저장 형식에서 재생성 | 모든 과거 snapshot 형식을 지원하지 않는다 |
| 실행 중인 PID | 원본 서버가 유지되면 유지 | 보장하지 않음 | 새 Space에는 새 프로세스가 생긴다 |
| 에이전트 대화 | 원본 에이전트가 유지되면 유지 | 유효한 참조와 제공자 지원이 있으면 재개 | 같은 프로세스의 메모리 복구와 다르다 |
| 실행 중인 명령 | 원본 서버가 유지되면 지속 | 이어받지 않음 | 테스트 진행률과 서버 메모리는 복원 대상이 아니다 |
| 셸 변수와 미저장 편집기 상태 | 원본 프로세스 안에 남음 | 자동 복원하지 않음 | 파일이나 snapshot에 없는 상태를 추정하지 않는다 |
| 스크롤백 | 원본 터미널이 보유한 내용 사용 | 초기 범위에서 이전 제외 | 배치 복원과 화면 기록 복원은 다르다 |
| 네트워크 장애 | 원본 서버 중지 명령을 보내지 않음 | 상태 조회 후 재개 | 호스트 장애나 OS 종료까지 막는 보장이 아니다 |

Herdr 0.9.5 문서는 재접속과 서버 재시작을 구분한다.
재접속은 살아 있는 프로세스를 사용한다.
서버 재시작은 저장된 구조를 복원하며 일반 프로세스를 이어받지 않는다.
native agent restore는 지원하는 대화 기록을 새 실행으로 재개한다.
[보존 범위](https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/session-state.mdx)를 확인했다.

### 원격 작업을 B에서 열기 {#REQ-connect toc="원격 작업을 B에서 열기"}

- 상태: proposed
- 근거: 사용자는 B에서 A의 기존 kanvibe 세션을 자신의 화면에 표시하기를 요청했다.
- 대상 사용자: B의 KanVibe와 외부 Herdr를 함께 사용하는 개발자다.
- 문제와 목표: A에서 실행하는 화면을 전달하는 현재 방식 대신 B의 클라이언트가 원격 작업을 구분하게 한다.
- 범위: SSH 대상과 실제 원격 세션을 등록하고 기존 Space를 선택한다.
- 제외 범위: A의 프로세스를 B로 이동하지 않는다.
- 성공 정의: B에서 A의 kanvibe Space에 입력이 전달되며 로컬 Space와 소유 호스트가 구분된다.
- 인수 조건: 기존 원격 PID, 작업 경로, 선택한 Space가 유지된다.
- 연결: [보존 규칙](#RULE-preserve), [연결 화면](#SCR-connect), [실행 계약](#EVT-execute), [검증 계획](#QA-transition)

### 분리된 기존 작업을 정리하기 {#REQ-rebuild toc="분리된 기존 작업을 정리하기"}

- 상태: proposed
- 근거: 사용자가 dotfiles-main 등 독립 세션 목록과 kanvibe 통합 목표를 제공했다.
- 대상 사용자: 과거 독립 세션을 사용하는 기존 KanVibe 사용자다.
- 문제와 목표: 원본 상태를 확인한 뒤 지원 가능한 작업만 kanvibe의 Space로 재구성한다.
- 범위: 동일 호스트의 stopped 원본에서 Space 하나를 선택해 경로와 지원하는 배치를 재생성한다.
- 제외 범위: running 원본의 자동 중지, 세션 간 PID 이동, 원본 자동 삭제는 제외한다.
- 성공 정의: 새 Space 검증 후 해당 태스크의 연결만 교체된다.
- 인수 조건: 원본이 남고 같은 요청의 중복 실행이 새 Space를 추가하지 않는다.
- 연결: [재구성 자격](#RULE-rebuild), [재구성 화면](#SCR-rebuild), [계획 계약](#EVT-plan), [검증 계획](#QA-transition)

### 실패해도 사용자가 현재 위치를 알기 {#REQ-recovery toc="실패해도 사용자가 현재 위치를 알기"}

- 상태: proposed
- 근거: 현재 machine 등록 실패는 terminal.ts에서 경고 로그만 남긴다.
- 대상 사용자: 연결 장애나 앱 종료를 만나는 개발자다.
- 문제와 목표: 연결 성공과 등록 성공을 분리하고 미완료 작업을 다시 조회하게 한다.
- 범위: 오류 단계, 원본과 대상 식별자, 저장 상태, 재시도 가능 여부를 표시한다.
- 제외 범위: 이미 실행된 명령의 외부 부수효과를 자동 취소하지 않는다.
- 성공 정의: 실패가 완료로 표시되지 않으며 원본과 신규 Space의 위치를 확인할 수 있다.
- 인수 조건: 앱을 재실행해도 미완료 operationId와 기존 태스크 연결을 판별한다.
- 연결: [실패 규칙](#RULE-recovery), [실패 화면](#SCR-recovery), [조회와 제어 계약](#EVT-operation), [검증 계획](#QA-transition)

## 2. 정책과 도메인: 실행 상태를 언제 보존할 수 있는가 {#policy toc="2. 정책과 도메인"}

### 현재 코드가 남겨 둔 세션 {#current-behavior toc="현재 코드가 남겨 둔 세션"}

현재 findAliveBranchSession은 공유 kanvibe Space를 먼저 찾는다.
공유 Space가 없으면 기존 독립 Herdr 세션을 재사용한다.
따라서 새 생성 방식을 바꿔도 기존 독립 세션은 자동으로 사라지지 않는다.
근거는 `src/desktop/main/services/projectService.ts`다.

현재 `kanban_tasks`는 `ssh_host`, `session_type`, `session_name`, `worktree_path`를 저장한다.
예를 들어 `kanvibe:dotfiles-main`은 서버 이름 전체가 아니다.
현재 `parseHerdrSessionTarget`은 서버 `kanvibe`와 작업 키 `dotfiles-main`으로 나눈다.
실제 workspace ID는 세션 디렉터리의 `kanvibe-workspaces.json`에서 읽는다.

| 사용자가 제공한 이름 | 제공 당시 상태 | 제안 처리 |
| --- | --- | --- |
| kanvibe | running | 대상 서버로 유지 |
| dotfiles-main, kanvibe-main | running | 원본 연결 유지, 재구성은 보류 |
| llm-wiki-mcp-main, nvim-master | running | 원본 연결 유지, 재구성은 보류 |
| eda-study-jibaek-atk-main | stopped | 저장 형식과 경로 검사 |
| fanplus-api-develop-develop, fanplus-api-develop-master | stopped | 태스크와 Space 대응 검사 |
| interlight2-feat-self-improving-prompt | stopped | 저장 형식과 경로 검사 |
| kanvibe-dev, kanvibe-fix-open-code-hooks-verification | stopped | 중복 Space와 태스크 대응 검사 |
| kanvibe-refactor-rust-tauri, prompt-main | stopped | 저장 형식과 경로 검사 |
| total-search-main, total-search-research-geo-v2 | stopped | 저장 형식과 경로 검사 |
| total-search-research-incident-alarm, total-search-research-mongodb-usage | stopped | 저장 형식과 경로 검사 |
| workspace-view-workspace-view-1-main, workspace-view-workspace-view-2-main | stopped | 태스크 대응 검사 |
| default | stopped | 개인 작업 여부를 확인하고 명시 선택한 경우만 검사 |

이 표는 사용자가 제공한 시점의 목록이다.
맥북의 현재 상태를 실시간으로 조회한 결과는 아니다.
이름이 비슷하다는 이유만으로 태스크와 세션을 자동 매칭하지 않는다.
동일한 task ID와 저장된 연결을 먼저 확인한다.

### 기존 실행을 보호하는 규칙 {#RULE-preserve toc="기존 실행을 보호하는 규칙"}

- 상태: proposed
- 근거: 사용자의 작업 보존 요구와 Herdr의 재접속 계약이다.
- 조건과 시점: 기존 세션에 연결하거나 running 원본을 검사할 때 적용한다.
- 판단 주체: Electron main의 연결 서비스가 최신 서버 상태를 확인한다.
- 결과: 원본 서버의 stop, delete, pane close, 프로세스 종료 명령을 실행하지 않는다.
- 실패와 재시도: 인증 실패는 사용자에게 표시하고 원본 연결을 유지한다.
- 검증: 원본 pane PID와 실행 표식이 연결 전후 일치하는지 확인한다.
- 결정: 없음

machine 프로필의 추가는 사용자에게 보이는 Herdr 설정 변경이다.
기존 프로필의 이름과 disabled 선택은 덮어쓰지 않는다.
이 기능의 활성화 범위를 설명하고 사용자 동작으로 등록한다.
검사 화면의 읽기 동작은 프로필을 추가하지 않는다.

### 재구성 자격을 확인하는 규칙 {#RULE-rebuild toc="재구성 자격을 확인하는 규칙"}

- 상태: proposed
- 근거: 실행 중인 프로세스의 세션 간 이동 API는 확인하지 못했다.
- 조건과 시점: rebuildStopped 계획 생성 시와 실행 직전에 다시 검사한다.
- 판단 주체: Electron main의 이전 서비스가 running 상태, 원본 지문, 경로 접근 권한을 확인한다.
- 결과: stopped이고 지원 형식이며 경로가 유효한 경우만 새 Space를 만든다.
- 실패와 재시도: running으로 바뀌거나 저장 내용이 달라지면 새 검사를 요구한다.
- 검증: 실행 직전 원본 변경을 주입하면 생성이나 태스크 갱신 전에 차단되어야 한다.
- 결정: 없음

하나의 이전 계획은 원본 Space 하나와 태스크 하나를 다룬다.
한 원본 세션에 Space가 여러 개 있으면 사용자가 각각 선택한다.
처음부터 여러 서버를 하나의 트랜잭션으로 묶지 않는다.
원본이 running이면 프로세스를 멈추는 자동 전환을 제공하지 않는다.
사용자가 별도로 작업을 종료한 뒤 새로운 검사를 해야 한다.
다른 터미널이 마지막 검사 직후 원본을 다시 시작할 가능성은 남는다.
KanVibe의 잠금은 외부 Herdr 실행까지 막지 못한다.
재개 전에도 중복 실행을 확인하고 불명확하면 attention으로 멈춘다.

### 대화를 재개할 때 지키는 규칙 {#RULE-resume toc="대화를 재개할 때 지키는 규칙"}

- 상태: proposed
- 근거: Herdr의 native agent restore는 유효한 제공자별 세션 참조가 필요하다.
- 조건과 시점: 재구성 완료 후 사용자가 대화 재개를 별도로 선택할 때 적용한다.
- 판단 주체: 제공자별 adapter가 참조 형식과 대상 호스트의 CLI 지원을 확인한다.
- 결과: 지원되는 에이전트 세션을 새 프로세스로 재개한다.
- 실패와 재시도: 참조가 없으면 정상 셸만 제공하고 재개 불가 사유를 표시한다.
- 검증: 같은 대화가 이미 실행 중이면 두 번째 에이전트를 시작하지 않는다.
- 결정: [지원 형식 결정](#DEC-support)

프로세스의 메모리와 실행 중인 명령은 재개 대상이 아니다.
저장된 임의 셸 명령을 자동 실행하지 않는다.
재개 명령은 제공자별 인자 배열로 만든다.
쉘 문자열을 원본 데이터에서 그대로 실행하지 않는다.
초기 지원 대상과 snapshot 형식은 검증 전까지 확정하지 않는다.

### 실패와 복구를 구분하는 규칙 {#RULE-recovery toc="실패와 복구를 구분하는 규칙"}

- 상태: proposed
- 근거: 원격 Space 생성과 로컬 SQLite 갱신은 하나의 원자적 쓰기가 아니다.
- 조건과 시점: 실행 중 오류, 앱 종료, 재시도, 복구 요청이 발생할 때 적용한다.
- 판단 주체: 영속 작업 기록과 실제 Herdr 상태를 대조하는 복구 서비스다.
- 결과: 원본은 보존하고 태스크 연결 변경 시점을 기록한다.
- 실패와 재시도: 동일 operationId를 재사용하며 기존 생성 대상을 먼저 찾는다.
- 검증: 원격 생성 직후와 DB 반영 직후에 종료시켜 각각 재시작한다.
- 결정: 없음

아래 그림은 제안 상태 전이다.
완료 전 실패와 완료 후 되돌리기의 의미가 다르다.

<figure src="diagrams/04-operation-state.svg">
제안 상태 전이: 실패 시 원본을 보존하고 불완전한 대상을 별도로 표시한다.
</figure>

원격 생성 직후에는 대상 workspaceId를 작업 기록에 남긴다.
기록 전에 앱이 종료될 수 있으므로 대상에 operationId 메타데이터도 남긴다.
원격 생성과 메타데이터 기록 사이의 종료는 자동 판별이 어려울 수 있다.
이 경우 생성을 반복하지 않고 attention 상태로 멈춘다.
사용자는 미등록 대상을 확인한 뒤 재개한다.
이는 전체 구간의 무조건적인 exactly-once 보장을 제공하지 않는다는 뜻이다.

### 검증 전에 확정하지 않을 판단 {#DEC-support toc="검증 전에 확정하지 않을 판단"}

- 상태: open
- 근거: 맥북의 실제 저장 파일과 제공자별 재개 상태는 아직 조사하지 않았다.
- 값: 미정
- 담당 역할: KanVibe 구현 담당자와 작업 소유자다.
- 판단 기준: 저장 형식의 손실 없는 해석, 제공자 CLI 지원, 중복 실행 방지다.
- 필요한 증거: 비식별 snapshot fixture, 제공자 세션 참조, macOS와 Linux의 실행 결과다.
- 결정 시점: 재구성 adapter와 resume 동작을 구현하기 전이다.

## 3. UX와 화면: 어떤 선택이 어떤 상태를 만드는가 {#ux toc="3. UX와 화면"}

### 화면을 읽는 방법 {#screen-guide toc="화면을 읽는 방법"}

아래 재생기는 문서의 설명용 상태 시뮬레이션이다.
버튼을 눌러도 Herdr나 KanVibe 데이터를 변경하지 않는다.
재생기의 저장 표는 그 단계와 관련된 상태만 골라 보여준다.
화면의 검사 상태와 영속 작업 기록을 같은 저장소로 간주하지 않는다.
실제 제품 목업은 아직 미제작이다.
스크린샷은 사용자가 제공한 Herdr 화면의 범위에서 확인했다.
`SettingsRoute.tsx`, `HooksStatusDialog.tsx`와 기존 디자인 토큰은 UI 연결 지점으로 확인했다.
제품 구현에서는 기존 설정 화면과 대화상자를 재사용한다.
문서의 회색 PRD 읽기 화면을 제품 디자인으로 복사하지 않는다.

### 연결과 이전을 직접 비교하기 {#transition-playbook toc="연결과 이전을 직접 비교하기"}

<playbook src="scenarios/transition.json">
시나리오를 선택하고 단계를 바꾸면 요청, 응답, 화면, 저장 상태를 함께 확인할 수 있다.
</playbook>

### 기존 세션 검사 화면 {#SCR-inventory toc="기존 세션 검사 화면"}

- 상태: proposed
- 근거: 사용자가 제공한 세션 목록과 SettingsRoute의 SSH 호스트 선택 구조다.
- 진입과 복귀: 설정의 Herdr 세션 관리 또는 태스크 상세의 기존 세션 연결에서 진입한다.
- 흐름: 호스트 선택, 읽기 검사, 세션 선택, Space 선택, 연결 또는 재구성 계획 확인 순서다.
- 화면 상태: 아래 상태표를 따른다.
- 목업: 실제 제품 목업 미제작이며 재생기는 상태 설명만 제공한다.
- 연결: [재구성 자격](#RULE-rebuild), [검사 계약](#EVT-scan), [검증 계획](#QA-transition)

| 상태 | 표시 | 허용 동작 | 저장 변화 |
| --- | --- | --- | --- |
| 최초 | 호스트 선택 | 검사 | 없음 |
| 로딩 | 검사 중, 대상 호스트 | 취소 | 없음 |
| 정상 | 상태, 경로, 태스크 연결, 지원 여부 | 연결, 계획 보기 | 없음 |
| 빈 결과 | 발견한 세션 없음 | 다시 검사, 돌아가기 | 없음 |
| 인증 거부 | SSH 인증 필요 | 인증 안내 후 재검사 | 없음 |
| 오프라인 | 마지막 결과는 오래된 결과로 표시 | 재검사 | 실행 버튼 비활성 |
| 지원 불가 | snapshot 형식 불명 또는 경로 없음 | 원본 유지 | 없음 |
| 앱 복귀 | 미완료 작업 존재 | 기록 조회 | 기존 기록 유지 |

### 기존 실행에 연결하는 화면 {#SCR-connect toc="기존 실행에 연결하는 화면" back="transition-playbook"}

- 상태: proposed
- 근거: 원격 서버와 클라이언트를 분리하는 Herdr machine 기능이다.
- 진입과 복귀: 검사 결과에서 attachExisting 계획을 연다.
- 흐름: 프로필 대조, 연결 준비, 소유 호스트 표시, 기존 Space 선택 순서다.
- 화면 상태: 연결 중, 연결 성공, 인증 필요, 원격 불가, disabled 선택 보존 상태를 표시한다.
- 목업: 실제 제품 목업 미제작이며 정상 연결과 인증 실패 시나리오를 제공한다.
- 연결: [원격 목표](#REQ-connect), [보존 규칙](#RULE-preserve), [실행 계약](#EVT-execute)

B의 화면에 A가 보이더라도 원본 실행은 A에 있다.
machine 등록 성공과 화면 연결 성공을 각각 표시한다.
등록만 성공했으면 원격 Space가 입력 가능한 상태라고 표시하지 않는다.
새 연결의 검증 실패는 현재 태스크 연결을 교체하지 않는다.

### 새 Space로 재구성하는 화면 {#SCR-rebuild toc="새 Space로 재구성하는 화면" back="transition-playbook"}

- 상태: proposed
- 근거: shared kanvibe workspace 생성과 작업 키 매핑이 현재 코드에 있다.
- 진입과 복귀: stopped 원본의 rebuildStopped 계획에서 진입한다.
- 흐름: 보존표 확인, 백업, 대상 생성, 검증, 태스크 연결 교체 순서다.
- 화면 상태: ready, blocked, applying, verifying, completed, attention을 표시한다.
- 목업: 실제 제품 목업 미제작이며 재구성과 실행 중 차단 시나리오를 제공한다.
- 연결: [재구성 목표](#REQ-rebuild), [재구성 자격](#RULE-rebuild), [계획 계약](#EVT-plan)

완료 화면에는 새 workspaceId와 기존 원본 위치를 함께 표시한다.
원본 세션은 자동 삭제하지 않는다.
따라서 session list에서 옛 이름이 남아도 이전 실패로 해석하지 않는다.
정리는 초기 기능 범위에서 제외한다.
정리 기능을 추가하려면 별도의 소유권과 사용 여부 검사가 필요하다.

### 실패와 되돌리기를 설명하는 화면 {#SCR-recovery toc="실패와 되돌리기를 설명하는 화면" back="transition-playbook"}

- 상태: proposed
- 근거: 로컬과 원격 쓰기 사이에 부분 완료가 발생할 수 있다.
- 진입과 복귀: 작업 기록, 실패 알림, 앱 복귀에서 진입한다.
- 흐름: 실제 상태 조회, 기존 연결 확인, 재시도 또는 연결 복구를 선택한다.
- 화면 상태: 실패 단계, 대상 존재 여부, DB 반영 여부, 중복 실행 위험을 구분한다.
- 목업: 실제 제품 목업 미제작이며 중간 실패와 재시작 복구 시나리오를 제공한다.
- 연결: [실패 규칙](#RULE-recovery), [조회와 제어 계약](#EVT-operation), [검증 계획](#QA-transition)

되돌리기는 우선 태스크의 연결 대상을 복구하는 동작이다.
실행한 명령이나 수정된 파일을 과거로 되돌리는 동작이 아니다.
대상에 새 작업이나 에이전트가 생겼으면 자동 종료하지 않는다.
원본 재연결이 중복 에이전트를 만들 수 있으면 복구를 차단하고 수동 확인을 요구한다.

## 4. API 계약: 어떤 요청이 어떤 저장을 허용하는가 {#api toc="4. API 계약"}

### 실제 IPC와 제안 API의 경계 {#api-boundary toc="실제 IPC와 제안 API의 경계"}

현재 renderer는 preload의 `invoke(namespace, method, args)`를 사용한다.
Electron main은 `kanvibe:invoke` 채널을 받아 `desktopServices`에 등록된 함수를 실행한다.
아래 `herdrSession` namespace는 아직 등록하지 않은 제안이다.
새 HTTP 서버나 외부 공개 API를 추가하는 계획이 아니다.

| 현재 구현 | 이 문서의 제안 |
| --- | --- |
| `kanvibe:invoke`와 desktopServices | 같은 IPC 경로에 herdrSession 서비스 추가 |
| Herdr 세션별 명령과 workspace 매핑 | 검사, 계획, 실행, 조회, 제어를 구분 |
| 태스크의 sessionName과 sshHost | 검증 후 기존 필드에 연결 대상 반영 |
| 세션 간 원자적 이동 계약 없음 | 단계별 기록과 보상 동작으로 부분 완료 관리 |

모든 요청은 객체 하나를 `args` 배열의 첫 항목으로 전달한다.
`sshHost: null`은 KanVibe를 실행하는 로컬 호스트다.
`sshHost: "dev-vm"`은 그 호스트의 SSH 설정 별칭이다.
UUID는 작업을 구분하는 문자열이다.
지문은 검사 당시 원본과 연결 상태에서 계산한 SHA-256 문자열이다.

공통 결과는 성공 또는 오류 중 하나다.
보안 정보와 원본 터미널 출력은 오류 메시지에 넣지 않는다.

```typescript
type Result<T> =
  | { ok: true; value: T }
  | { ok: false; error: {
      code: string;
      message: string;
      retryable: boolean;
      operationId: string | null;
    }};
```

알 수 없는 오류 code는 일반 실패로 표시한다.
알 수 없는 phase는 쓰기를 멈추고 업데이트 필요로 표시한다.
오류를 받았다는 이유만으로 동일 쓰기를 새로운 operationId로 다시 실행하지 않는다.

### 읽기 전용 세션 검사 {#EVT-scan toc="읽기 전용 세션 검사"}

- 상태: proposed
- 근거: 현재 listHerdrSessions, parseSSHConfig, getTaskRepository를 조합할 수 있다.
- 이벤트/토픽: IPC herdrSession.scanSessions
- 발행과 소비: renderer가 preload를 통해 요청하고 Electron main의 제안 서비스가 소비한다.
- 스키마: 이 항목의 요청과 응답 필드를 정본으로 사용하며 Result envelope를 적용한다.
- 요청: [검사 필드](#scan-fields)의 객체 하나다.
- 응답: Result 안의 scanId, observedAt, fingerprint, sessions다.
- 오류: SSH_AUTH_REQUIRED, HOST_UNREACHABLE, HERDR_UNAVAILABLE, READ_FAILED다.
- 누락/null: sshHost는 필수이며 null만 로컬을 뜻한다. 원본 세션 디렉터리를 읽지 못하면 null과 snapshotState를 함께 반환한다.
- 기본값: 암묵적인 세션 생성이나 설치 기본값은 없다.
- 호환성: 기존 태스크 필드와 원격 프로필을 변경하지 않는다.
- 재시도와 중복: 읽기 재시도는 허용한다. 각 결과에 새 scanId를 준다.
- 연결: [검사 화면](#SCR-inventory), [재구성 자격](#RULE-rebuild), [검증 계획](#QA-transition)

### 검사 필드와 결과 {#scan-fields toc="검사 필드와 결과"}

| 필드 | 타입과 필수성 | 값과 조건 |
| --- | --- | --- |
| request.sshHost | string 또는 null, 필수 | 사용 가능한 SSH 별칭 또는 로컬 |
| value.scanId | UUID string, 필수 | 해당 앱 실행의 검사 결과 식별자 |
| value.observedAt | ISO 8601 string, 필수 | UTC 시각이며 Z로 끝남 |
| value.fingerprint | string, 필수 | 원본 목록과 연결 상태의 SHA-256 |
| value.sessions | ScanSession 배열, 필수 | 빈 배열은 정상 빈 결과 |
| ScanSession.name | string, 필수 | Herdr가 반환한 정확한 이름 |
| ScanSession.running | boolean, 필수 | 검사 시점 상태이며 실행 직전 재확인 |
| ScanSession.sessionDir | string 또는 null, 필수 | 원본 호스트의 경로, 읽지 못하면 null |
| ScanSession.snapshotState | valid, missing, unsupported, unreadable 중 하나, 필수 | 지원 여부는 decoder 검증 결과 |
| ScanSession.workspaceIds | string 배열, 필수 | 확인한 Space 식별자만 포함 |
| ScanSession.taskIds | UUID string 배열, 필수 | 기존 저장 연결로 확인한 태스크만 포함 |

### 변경 없는 계획 작성 {#EVT-plan toc="변경 없는 계획 작성"}

- 상태: proposed
- 근거: 검사와 쓰기를 나누어 원본 보존 조건을 사용자에게 제시해야 한다.
- 이벤트/토픽: IPC herdrSession.createPlan
- 발행과 소비: renderer가 preload를 통해 요청하고 Electron main의 제안 서비스가 소비한다.
- 스키마: 이 항목의 요청과 응답 필드를 정본으로 사용하며 Result envelope를 적용한다.
- 요청: scanId, taskId, sourceSession, sourceWorkspaceId, mode를 모두 받는다.
- 응답: Result 안에 planId, fingerprint, targetSession, eligibility, preserved, recreated, unsupported, blockers를 반환한다.
- 오류: SCAN_NOT_FOUND, TASK_SOURCE_MISMATCH, SOURCE_RUNNING, SNAPSHOT_UNSUPPORTED, CWD_UNAVAILABLE, TARGET_CONFLICT다.
- 누락/null: 입력 다섯 필드는 null과 누락을 거부한다. mode는 attachExisting 또는 rebuildStopped다.
- 기본값: targetSession은 rebuildStopped에서 kanvibe로 고정한다. attachExisting에서는 sourceSession을 유지한다.
- 호환성: taskId가 기존 Herdr 태스크인지 검사한다. 다른 sessionType을 자동 변환하지 않는다.
- 재시도와 중복: 파일, DB 연결, machine 프로필에 쓰지 않는다.
- 연결: [재구성 화면](#SCR-rebuild), [재구성 자격](#RULE-rebuild), [검증 계획](#QA-transition)

| 결과 필드 | 타입과 단위 | 의미 |
| --- | --- | --- |
| planId | UUID string | 계획 식별자 |
| fingerprint | string | 검사 결과와 태스크 연결의 지문 |
| targetSession | string | 실제 목표 Herdr 서버 이름 |
| eligibility | ready 또는 blocked | 실행 허용 여부 |
| preserved | string 배열 | 원본에서 계속 사용하는 상태 |
| recreated | string 배열 | 새로 만드는 상태 |
| unsupported | string 배열 | 그대로 이전하지 못하는 상태 |
| blockers | 오류 code 배열 | 하나라도 있으면 실행 차단 |

계획 입력에서 `sourceSession: "dotfiles-main"`은 실제 서버 이름이다.
새 연결값 `sessionName: "kanvibe:dotfiles-main"`과 혼동하지 않는다.
지원 형식이 미정이면 readiness를 확인할 수 없다.
실제 decoder 검증 전에는 ready를 반환하지 않는다.

### 계획의 실행 {#EVT-execute toc="계획의 실행"}

- 상태: proposed
- 근거: 원격 쓰기와 로컬 갱신 사이의 작업 기록이 필요하다.
- 이벤트/토픽: IPC herdrSession.executePlan
- 발행과 소비: renderer가 preload를 통해 요청하고 Electron main의 제안 서비스가 소비한다.
- 스키마: 이 항목의 요청과 응답 필드를 정본으로 사용하며 Result envelope를 적용한다.
- 요청: planId, expectedFingerprint, operationId를 받는다. 앞의 둘은 string이며 operationId는 UUID string이다.
- 응답: Result 안의 operationId와 phase를 반환한다. 완료를 기다리는 동안에도 조회할 수 있다.
- 오류: SOURCE_CHANGED, SOURCE_RUNNING, PLAN_BLOCKED, TARGET_CONFLICT, CONNECT_FAILED, VERIFY_FAILED, OPERATION_CONFLICT다.
- 누락/null: 세 필드는 필수이며 null을 거부한다. 원본과 대상 식별자는 planId에서 읽는다.
- 기본값: 대화 재개는 자동 실행하지 않는다. 원본 세션 삭제도 실행하지 않는다.
- 호환성: 기존 태스크 연결은 대상 검증 전까지 유지한다.
- 재시도와 중복: 같은 operationId와 같은 입력은 기존 작업을 반환한다. 다른 입력 재사용은 OPERATION_CONFLICT다.
- 연결: [보존 규칙](#RULE-preserve), [실패 규칙](#RULE-recovery), [실패 화면](#SCR-recovery)

실행은 원본 상태를 다시 읽는다.
계획 이후 원본이 실행되었으면 쓰기를 차단한다.
일시적인 SSH 오류 뒤에 원격 생성 성공 여부를 모르면 attention으로 멈춘다.
결과를 모르는 쓰기를 무조건 반복하지 않는다.

### 작업 상태 조회 {#EVT-operation toc="작업 상태 조회"}

- 상태: proposed
- 근거: 실행 중 앱 종료와 부분 완료를 구분해야 한다.
- 이벤트/토픽: IPC herdrSession.getOperation
- 발행과 소비: renderer가 preload를 통해 요청하고 Electron main의 제안 서비스가 소비한다.
- 스키마: 이 항목의 요청과 응답 필드를 정본으로 사용하며 Result envelope를 적용한다.
- 요청: operationId UUID string 하나를 가진 객체다.
- 응답: [영속 작업 기록](#operation-fields)의 공개 필드를 반환한다.
- 오류: OPERATION_NOT_FOUND, READ_FAILED다.
- 누락/null: source와 phase는 필수다. 아직 생성하지 않은 targetWorkspaceId와 실패하지 않은 error는 null이다.
- 기본값: 조회만으로 Space 생성이나 원본 시작을 실행하지 않는다.
- 호환성: 알 수 없는 phase는 안전한 조회 화면에서 표시하며 실행을 막는다.
- 재시도와 중복: 조회는 부수효과가 없다.
- 연결: [실패 규칙](#RULE-recovery), [실패 화면](#SCR-recovery), [검증 계획](#QA-transition)

### 취소와 연결 복구 {#EVT-control toc="취소와 연결 복구"}

- 상태: proposed
- 근거: 연결 복구와 명령의 실행 취소를 구분해야 한다.
- 이벤트/토픽: IPC herdrSession.controlOperation
- 발행과 소비: renderer가 preload를 통해 요청하고 Electron main의 제안 서비스가 소비한다.
- 스키마: 이 항목의 요청과 응답 필드를 정본으로 사용하며 Result envelope를 적용한다.
- 요청: operationId UUID string과 action enum을 받는다. action은 cancel 또는 revertBinding이다.
- 응답: 변경된 phase와 다음에 가능한 사용자 동작을 문자열 배열로 반환한다.
- 오류: NOT_CANCELLABLE, REVERT_BLOCKED, BINDING_CHANGED, OPERATION_NOT_FOUND다.
- 누락/null: 두 필드는 필수이며 null을 거부한다.
- 기본값: 대상 프로세스 종료, 파일 변경 취소, 원본 삭제는 하지 않는다.
- 호환성: 해당 작업이 아직 소유하는 태스크 연결만 compare-and-set으로 복구한다.
- 재시도와 중복: 같은 취소와 복구는 기존 결과를 반환한다. 새 변경을 덮어쓰지 않는다.
- 연결: [실패 규칙](#RULE-recovery), [실패 화면](#SCR-recovery), [검증 계획](#QA-transition)

### 오류가 화면으로 이어지는 방식 {#error-map toc="오류가 화면으로 이어지는 방식"}

| 오류 | 발생 조건 | 화면과 복귀 | 재시도 조건 |
| --- | --- | --- | --- |
| SSH_AUTH_REQUIRED | SSH 인증 거부 | 인증 안내, 원본 연결 유지 | 인증을 해결한 뒤 재검사 |
| HOST_UNREACHABLE | 호스트 응답 불가 | 오프라인, 오래된 목록 표시 | 연결 복구 뒤 재검사 |
| SOURCE_RUNNING | 재구성 원본 실행 중 | 이전 보류, 기존 연결 선택 가능 | 원본 중단 후 새 검사 |
| SOURCE_CHANGED | 지문 또는 실행 상태 변경 | 기존 계획 만료 표시 | 새 계획 생성 |
| SNAPSHOT_UNSUPPORTED | decoder가 원본 형식 미지원 | 재구성 불가, 원본 유지 | 지원 adapter 검증 후 재검사 |
| VERIFY_FAILED | 경로, pane, 배치 확인 실패 | 대상 보류, 기존 연결 유지 | 부분 완료 대조 후 재개 |
| TARGET_CONFLICT | 같은 task의 대상이 이미 존재 | 기존 대상 검사 | 새 중복 생성 없이 대조 |
| REVERT_BLOCKED | 새 에이전트 또는 변경된 연결 존재 | 수동 확인 필요 | 새 작업 소유권 확인 |

## 5. 기술 설계: 화면과 실행의 소유자는 누구인가 {#technical toc="5. 기술 설계"}

### 현재 연결에서 원격 화면이 최상위인 이유 {#TECH-current toc="현재 연결에서 원격 화면이 최상위인 이유"}

- 상태: current
- 근거: src/lib/terminal.ts의 spawnRemoteSession, prepareHerdrTaskClient와 src/lib/herdr.ts의 buildHerdrClientCommand다.
- 책임과 경계: KanVibe가 SSH PTY를 열고 원격 호스트에서 Herdr client를 실행한다.
- 처리와 저장: 공유 Space는 원격 socket을 프록시로 연결하고 해당 workspaceId를 선택한다.
- 정합성과 경쟁: 현재 프록시는 client 연결별로 workspace.focus를 보낸다.
- 캐시와 작업 큐: 현재 machine 등록은 별도 비동기 경로이며 실패는 경고 로그로 남는다.
- 결정과 비용: B에서 A의 UI를 받으므로 A 기준 Space 목록을 보게 된다.

현재 동작의 기준은 조사한 로컬 코드다.
맥북에 배포된 바이너리와 버전이 같은지는 확인하지 않았다.
아래 그림의 A와 B는 역할 예시이며 실제 호스트명과 무관하다.

<figure src="diagrams/01-current-topology.svg">
현재 로컬 코드: 실행과 Herdr client가 모두 A에 있어 B는 A 기준 화면을 전달받는다.
</figure>

### 제안한 연결에서 바뀌는 책임 {#TECH-boundary toc="제안한 연결에서 바뀌는 책임"}

- 상태: proposed
- 근거: Herdr saved machine 기능과 사용자가 요청한 B의 통합 탐색 화면이다.
- 책임과 경계: B는 UI와 machine catalog를 소유한다. A는 서버와 실행 중인 작업을 소유한다.
- 처리와 저장: B의 기존 프로필을 대조하고 실제 A 세션에 연결한다.
- 정합성과 경쟁: taskId, 호스트, 실제 서버, workspaceId를 함께 사용해 서로 다른 서버의 w1 충돌을 막는다.
- 캐시와 작업 큐: machine 등록은 호스트와 세션 기준으로 직렬화하고 상태를 화면에 전달한다.
- 결정과 비용: task별 Space 선택을 B의 client에서 유지하는 기술 검증이 먼저 필요하다.

제안 그림의 핵심은 UI가 B로 옮겨간다는 점이다.
A의 서버, 작업 디렉터리, 프로세스는 이동하지 않는다.
그림의 저장 항목은 서로 다른 소유 범위다.

<figure src="diagrams/02-federated-topology.svg">
변경 후 제안이며 미구현이다. B가 Local과 A를 구분하고 A의 기존 실행에 연결한다.
</figure>

현재 프록시의 workspace.focus는 하나의 서버 연결을 대상으로 한다.
이를 로컬 다중 machine UI의 remote 선택과 같다고 간주하면 안 된다.
첫 구현 단계에서 원격 endpoint 선택과 client별 Space 선택을 검증한다.
Herdr의 공개 계약만으로 제어할 수 없으면 제품 기능을 약속하지 않고 설계를 수정한다.
외부 Herdr에서 수동으로 machine을 선택하는 기능과 KanVibe의 자동 선택은 별도 인수 조건이다.

### 중단된 작업을 새 서버 구조로 만드는 방식 {#TECH-rebuild toc="중단된 작업을 새 서버 구조로 만드는 방식"}

- 상태: proposed
- 근거: 현재 ensureHerdrSession과 createHerdrWorkspace는 공유 서버에 새 Space를 만들 수 있다.
- 책임과 경계: decoder는 원본 snapshot을 읽는다. 실행 서비스는 지원하는 배치만 생성한다.
- 처리와 저장: 백업, 경로 검증, 대상 생성, 배치 검증, 연결 반영을 단계별로 기록한다.
- 정합성과 경쟁: 원격 workspace 매핑 파일과 SQLite 연결은 분리된 쓰기이므로 실패 보상과 재대조가 필요하다.
- 캐시와 작업 큐: 같은 원본과 task의 동시 실행은 잠근다. 임의 TTL 대신 현재 지문을 다시 검사한다.
- 결정과 비용: 새 API와 version별 snapshot decoder가 필요하며 손실 없는 전체 snapshot 복사로 간주하지 않는다.

아래 그림에서 원본 snapshot은 읽는 입력이다.
같은 작업 디렉터리는 파일 보존의 기준이다.
새 Space의 프로세스는 원본 PID와 다르다.

<figure src="diagrams/03-rebuild-boundary.svg">
변경 후 제안이며 미구현이다. 경로와 지원 배치를 재구성하며 원본 세션과 파일은 보존한다.
</figure>

기존 `session.json`을 대상 서버의 파일 위에 덮어쓰지 않는다.
서로 다른 서버의 `w1` ID를 그대로 합치지 않는다.
표시 이름 변경은 실제 이전과 다르다.
완료 판정은 태스크와 새 workspaceId의 연결까지 확인한다.

### 영속 작업 기록에 필요한 값 {#operation-fields toc="영속 작업 기록에 필요한 값"}

새 저장소는 로컬 SQLite에 추가하는 제안이다.
신규 테이블의 이름은 미정이다.
현재 `kanban_tasks`와 별개로 원본과 대상의 대응을 저장해야 한다.
아래는 애플리케이션의 제안 필드이며 실제 DB 컬럼명은 아니다.

| 필드 | 타입과 필수성 | 소유와 수명 |
| --- | --- | --- |
| operationId | UUID string, 필수 | 중복 실행을 구분하는 로컬 작업 키 |
| planId | UUID string, 필수 | 실행한 계획 |
| taskId | UUID string, 필수 | 연결을 교체할 기존 태스크 |
| mode | attachExisting 또는 rebuildStopped, 필수 | 수행 방식 |
| source | HostSessionWorkspace 객체, 필수 | sshHost, sessionName, workspaceId 포함 |
| sourceFingerprint | SHA-256 string, 필수 | 검사 결과와 실행 직전 상태 비교 |
| previousBinding | TaskBinding 객체, 필수 | sshHost, sessionType, sessionName, worktreePath 포함 |
| targetWorkspaceId | string 또는 null, 필수 | 생성 전 null |
| phase | OperationPhase enum, 필수 | 아래 상태 목록 |
| error | 오류 객체 또는 null, 필수 | 정상 상태는 null |
| createdAt, updatedAt | UTC ISO 8601 string, 필수 | 조사와 복구 순서 확인 |
| backupLocation | string 또는 null, 필수 | 소유 호스트 안의 백업 경로 |

`HostSessionWorkspace.sshHost`는 string 또는 null이다.
`sessionName`과 `workspaceId`는 비어 있지 않은 string이다.
`TaskBinding`은 현재 KanbanTask에 있는 필드와 같은 의미다.
`OperationPhase`는 planned, applying, verifying, committed, completed, failed, attention, cancelled, reverted다.
기록과 백업의 보존 기간은 [출시 결정](#DEC-retention)에서 정한다.

### 실행 중인 상태를 유지하는 대안 {#running-alternative toc="실행 중인 상태를 유지하는 대안"}

running 원본을 반드시 유지해야 하면 기존 서버에 연결한다.
그 세션이 실제로 kanvibe 서버에 합쳐졌다고 표시하지 않는다.
선택한 legacy 세션은 별도의 원격 프로필로 노출할 수 있다.
같은 호스트의 프로필 여러 개는 세션을 구분하는 이름을 사용한다.
이름은 사용자가 기존에 정한 값을 우선한다.

원본 작업을 kanvibe의 새 pane 안에서 중계하는 방식은 추가 연구 대상이다.
그 경우도 실제 실행 서버는 legacy 서버다.
legacy 서버를 종료하면 중계도 유지되지 않는다.
이 문서는 중계 방식의 구현과 성능을 보장하지 않는다.

Herdr의 실험적 live handoff는 서버 교체를 위한 기능이다.
독립 세션 여러 개를 하나로 합치는 지원 계약으로 사용하지 않는다.
[handoff 범위](https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/session-state.mdx)를 구분한다.

### 플랫폼별로 달라지는 부분 {#PLAT-backend toc="플랫폼별로 달라지는 부분"}

- 상태: proposed
- 근거: Electron main 서비스, SQLite 태스크 저장, SSH 실행 경로가 현재 코드에 있다.
- 공통 원문: [책임 경계](#TECH-boundary), [재구성 처리](#TECH-rebuild), [실행 계약](#EVT-execute)
- 차이: 여기서 backend는 공개 서버가 아니라 Electron main을 뜻한다. macOS와 Linux의 파일 권한과 Unix socket 경로를 각각 검증한다.
- 수명과 복원: 작업 기록을 앱 재시작 뒤 읽고 원격 상태와 대조한다.
- 권한과 진입: 사용자가 선택한 호스트와 기존 태스크의 소유 연결만 허용한다. 개인키 내용은 기록하지 않는다.
- 검증: macOS 로컬, macOS에서 Linux 원격, Linux 로컬 fixture를 사용한다.

### 화면에서 지켜야 할 차이 {#PLAT-web toc="화면에서 지켜야 할 차이"}

- 상태: proposed
- 근거: 현재 UI는 Electron renderer의 React 화면과 preload IPC를 사용한다.
- 공통 원문: [검사 화면](#SCR-inventory), [연결 화면](#SCR-connect), [실패 화면](#SCR-recovery)
- 차이: 여기서 web은 renderer다. 브라우저에서 단독으로 여는 제품 기능을 추가하지 않는다.
- 수명과 복원: 화면을 닫아도 operationId로 진행 상태를 다시 조회한다.
- 권한과 진입: 로딩 중 반복 실행을 막고 키보드로 취소와 상세 열기를 제공한다.
- 검증: 모달 초점 복귀, 다중 창의 Space 선택, 실패 후 새로고침 상태를 검증한다.

Mobile의 새 세션 관리 UI는 이번 범위에서 제외한다.
기존 Mobile 연결이 쓰는 attachTaskTerminal의 동작은 회귀 검사 대상으로 남긴다.

## 6. 검증과 개발 순서: 어떤 증거가 있어야 다음으로 가는가 {#validation toc="6. 검증과 개발 순서"}

### 실제 기능의 검증 계획 {#QA-transition toc="실제 기능의 검증 계획"}

- 상태: proposed
- 근거: 현재 herdr.test.ts, herdrClientProxy.test.ts와 데스크톱 테스트 구조다.
- 대상: [원격 연결](#REQ-connect), [재구성](#REQ-rebuild), [실패 복구](#REQ-recovery)의 인수 조건이다.
- 환경: Node 24와 프로젝트 Vitest, Herdr 0.9.5 fixture, macOS B와 Linux A다.
- 절차: 아래 단계별 검증표에서 실패 입력을 먼저 실행한다.
- 기대 결과: 미정
- 증거: 구현 현황의 제품 검증과 문서 재생기 검사는 구분한다. 실제 사용자의 세션 이전은 실행하지 않았다.
- 결정: [지원 형식 결정](#DEC-support)

지원 형식을 확정하기 전까지 실제 제품의 기대 결과는 미정이다.
아래는 구현 전에 검토할 검증 설계 제안이다.

| 단계 | 구현할 내용 | 진행을 허용하는 증거 | 멈춰야 할 조건 |
| --- | --- | --- | --- |
| P0 기능 경계 검증 | B의 local client에서 A machine과 task별 Space 선택 검증 | 두 창이 서로 다른 Space를 보며 선택을 덮어쓰지 않음 | 공개 제어 방식으로 선택 불가 |
| P1 읽기 검사 | 목록, 저장 형식, 경로, 태스크 대응 수집 | 검사 전후 파일과 PID와 DB가 동일 | 검사만으로 원본 서버가 시작됨 |
| P2 기존 연결 | 명시 동작으로 프로필 등록과 재연결 | 기존 설정 보존, 인증 오류 표시, 실제 입력 확인 | 등록 성공을 연결 성공으로 표시 |
| P3 재구성 adapter | 지원 형식 fixture의 경로와 배치 재생성 | 미지원 형식 거부, running 검사 차단 | 원본을 중지하거나 원본 파일 덮어씀 |
| P4 기록과 복구 | 지문, 잠금, 단계 기록, 조건부 연결 반영 | crash 주입 후 중복 생성 없이 대조하거나 attention | 결과 불명 쓰기를 자동 반복 |
| P5 대화 재개 | 검증된 제공자만 별도 동작으로 재개 | 정확한 session ID와 중복 에이전트 차단 | 임의 snapshot 명령 실행 |
| P6 실제 앱 통합 | 설정과 태스크 상세에 상태와 오류 표시 | 다중 창, 외부 Herdr, 기존 Mobile 회귀 통과 | 기존 작업 종료 또는 잘못된 호스트 입력 |

개발 순서는 P0부터 P6까지다.
P0가 실패하면 로컬 다중 machine 자동 선택의 범위를 다시 정한다.
문서에 그린 목표 화면만으로 기술 검증을 통과했다고 판단하지 않는다.

### 코드 변경 지점 {#implementation-map toc="코드 변경 지점"}

| 위치 | 현재 역할 | 제안 변경 |
| --- | --- | --- |
| src/lib/herdr.ts | 서버, Space, machine 명령 | 검사와 명시 등록 결과를 typed 결과로 제공 |
| src/lib/terminal.ts | 로컬과 SSH PTY 생성 | local client 경로와 기존 프록시의 책임 분리 |
| src/lib/herdrClientProxy.ts | client별 workspace 선택 | 다중 endpoint 선택 지원 여부를 P0에서 검증 |
| src/desktop/main/services/projectService.ts | 기존 branch 세션 탐색 | legacy 연결은 검사 대상으로 표시하고 자동 종료 금지 |
| src/desktop/main/serviceRegistry.ts | IPC namespace 등록 | herdrSession 제안 서비스 등록 |
| src/desktop/main/services | main 서비스 | 검사와 이전을 위한 신규 파일, 파일명은 구현 때 확정 |
| src/entities/KanbanTask.ts | 현재 연결 필드 | 기존 필드 의미 유지, 조건부 연결 갱신 |
| src/desktop/renderer/routes/SettingsRoute.tsx | 설정 화면 | 세션 관리 진입점 |
| 기존 단위와 통합 테스트 | Herdr 명령과 프록시 검증 | 원본 보존, 실패 주입, 재시작 복구 테스트 |
| docs와 다국어 안내 | 연결 방식 설명 | 등록과 연결과 재구성을 구분한 안내 |

### 실제로 확인할 실패 사례 {#failure-tests toc="실제로 확인할 실패 사례"}

| 사례 | 기대하는 제품 동작 제안 | 확인 방법 |
| --- | --- | --- |
| disabled 프로필 | 그대로 유지하고 사용자 선택 필요 표시 | profile JSON 전후 비교 |
| 원격 workspace ID 충돌 | 호스트와 서버 이름으로 구분 | 두 서버에 w1 생성 |
| 원본이 실행 중으로 변경 | 재구성 차단 | 계획 후 원본 시작 |
| 원격 생성 뒤 통신 단절 | 새 생성 반복 금지 | 응답을 끊고 상태 재조회 |
| 생성 뒤 메타데이터 기록 전 종료 | attention 표시 | 해당 구간 crash 주입 |
| DB 반영 직후 종료 | committed 상태 대조 | 앱 재시작 뒤 실제 연결 확인 |
| 다른 창이 태스크 연결 변경 | BINDING_CHANGED 표시 | compare-and-set 충돌 주입 |
| 새 대상에 사용자 작업 발생 | 자동 정리와 강제 복구 차단 | 작업 시작 후 복구 요청 |
| snapshot 경로 사라짐 | 경로 오류 표시 | 지원 fixture의 디렉터리 삭제 |
| resume 참조 누락 | 정상 셸만 제공 | 참조 없는 fixture |
| 원격 프로필 등록 실패 | 기존 연결 유지와 오류 표시 | SSH 인증 거부 |
| 문서 재생기 | 실제 쓰기 없이 상태만 변경 | 요청 패널과 외부 요청 감시 |

### 이번 미리보기의 검증 범위 {#preview-verification toc="이번 미리보기의 검증 범위"}

문서 구조 검사, 내부 링크, SVG, 재생기, 좁은 화면, 오프라인 HTML을 확인한다.
제품 구현과 macOS의 실제 세션 이전은 실행하지 않는다.
검사 결과와 실행 환경은 동봉하는 validation.json에 남긴다.
문서 원문과 그림과 시나리오 JSON이 같은 상태를 뜻하는지도 대조한다.

## 7. 출시와 운영: 기존 작업을 언제 전환할 것인가 {#release toc="7. 출시와 운영"}

### 읽기 검사부터 배포하기 {#REL-rollout toc="읽기 검사부터 배포하기"}

- 상태: proposed
- 근거: 기존 세션에는 실행 중인 작업과 저장된 복구 자료가 함께 존재한다.
- 배포와 전환: 읽기 검사, 기존 연결, stopped 재구성, 검증된 제공자 재개 순서로 기능을 연다.
- 버전 공존: 이전 앱은 기존 sessionName을 계속 사용할 수 있다. 신규 작업 기록은 새 앱만 해석한다.
- 관측: operationId, host 식별자, 단계, 오류 code, 복구 필요 여부를 기록한다.
- 진행과 중단: 원본 보존과 재시작 복구를 검증하면 진행한다. 잘못된 호스트 연결이나 중복 생성이 발견되면 쓰기 기능을 중단한다.
- 복구: 신규 실행을 막고 기존 세션 연결을 유지한다. 원본 세션 삭제는 출시 범위에 포함하지 않는다.

feature flag의 실제 이름과 저장 위치는 구현 때 기존 appSettings를 따른다.
renderer의 일회성 상태에 전체 앱의 기능 설정을 저장하지 않는다.
기존 원본이 존재한다는 사실만으로 실패나 사용하지 않는 데이터라고 분류하지 않는다.
이전 후 새 Space에 작업이 시작되면 앱 버전 롤백이 그 작업을 원상복구하지 않는다.

### 보존 기간을 확정할 시점 {#DEC-retention toc="보존 기간을 확정할 시점"}

- 상태: open
- 근거: 사용자가 원본과 작업 기록의 보존 기간을 지정하지 않았다.
- 값: 미정
- 담당 역할: 제품 담당자와 작업 소유자다.
- 판단 기준: 사용자의 복구 가능 시간, 로컬 저장 비용, 민감한 경로 기록의 보존 범위다.
- 필요한 증거: 실제 snapshot 크기와 원본 정리 요구다.
- 결정 시점: 자동 정리나 만료 기능을 구현하기 전이다.

자동 만료와 자동 원본 삭제는 현재 제안에 없다.
기간이 미정이라는 이유로 임의의 보존 일수를 넣지 않는다.
진단 로그에 SSH 개인키, 대화 본문, 환경 변수 전체를 기록하지 않는다.

## 부록: 용어를 같은 뜻으로 읽기 {#glossary toc="부록: 용어"}

<term id="herdr-session" title="Herdr 세션(Herdr session)" aliases="Herdr 세션">
Herdr 세션은 한 서버가 관리하는 독립 작업 단위다.

존재 이유: 서버마다 작업 공간과 실행 상태를 분리한다.

구체 예: `dotfiles-main`과 `kanvibe`는 서로 다른 Herdr 세션이다.

혼동 주의: 에이전트의 대화 기록을 뜻하는 에이전트 세션과 다르다.
</term>

<term id="space" title="Space(workspace)" aliases="Space|workspace ID|workspaceId">
Space는 Herdr 세션 안의 프로젝트 작업 공간이다.

존재 이유: 한 서버 안에서 여러 작업의 탭과 터미널을 구분한다.

구체 예: `kanvibe` 서버 안의 `dotfiles-main` 작업 공간이다.

혼동 주의: `w1` 같은 식별자는 다른 서버에도 존재할 수 있다.
</term>

<term id="machine" title="machine 프로필(saved SSH machine)" aliases="machine 프로필">
machine 프로필은 원격 호스트와 원격 Herdr 세션을 연결하는 저장 정보다.

존재 이유: 로컬 화면에서 원격 작업을 찾아 연결한다.

구체 예: `dev-vm` 호스트의 `kanvibe` 세션이다.

혼동 주의: 프로필 등록은 세션 병합이나 접속 성공을 뜻하지 않는다.
</term>

<term id="agent-session" title="에이전트 세션(agent session)" aliases="에이전트 세션">
에이전트 세션은 코딩 도구가 저장한 대화 기록이다.

존재 이유: 지원하는 도구가 이전 대화를 다시 열 수 있다.

구체 예: 기존 Codex 대화를 식별하는 session ID다.

혼동 주의: 에이전트 세션을 재개해도 이전 프로세스의 메모리는 돌아오지 않는다.
</term>

<term id="process" title="프로세스(process)" aliases="프로세스">
프로세스는 운영체제가 실행하는 프로그램의 한 인스턴스다.

존재 이유: 명령과 서버와 에이전트가 메모리를 갖고 실행된다.

구체 예: 원본 pane에서 실행 중인 테스트 프로그램이다.

혼동 주의: PID는 프로세스 식별자이며 파일이나 대화 ID와 다르다.
</term>

<term id="pane" title="pane" aliases="pane">
pane은 Herdr의 한 터미널 영역이다.

존재 이유: 탭 안에서 여러 셸의 입력과 출력을 나눈다.

구체 예: `w1:p1`이다.

혼동 주의: pane의 화면을 재생성해도 이전 프로세스가 이동하지 않는다.
</term>

<term id="snapshot" title="snapshot" aliases="snapshot">
snapshot은 Herdr가 저장한 작업 구조와 복구 정보다.

존재 이유: 서버가 종료된 뒤 지원하는 구조를 복원한다.

구체 예: 세션 디렉터리의 `session.json`이다.

혼동 주의: snapshot은 일반 프로세스의 메모리 복사본이 아니다.
</term>

<term id="resume" title="resume" aliases="resume">
resume은 지원하는 에이전트 대화를 새 실행에서 다시 여는 동작이다.

존재 이유: 이전 대화의 맥락을 새 프로세스에서 사용한다.

구체 예: 유효한 session ID를 제공자 CLI에 전달한다.

혼동 주의: 실행 중이던 임의 명령을 중간 위치부터 이어주는 기능은 아니다.
</term>

<term id="ipc" title="프로세스 간 통신(Inter-Process Communication, IPC)" aliases="IPC">
IPC는 앱 안의 서로 다른 프로세스가 요청을 주고받는 통신이다.

존재 이유: renderer가 main의 시스템 기능을 호출한다.

구체 예: `kanvibe:invoke` 채널이다.

혼동 주의: 이 문서의 서비스 method는 HTTP URL이 아니다.
</term>

<term id="fingerprint" title="지문(fingerprint)" aliases="지문">
지문은 검사한 데이터의 변경을 감지하기 위한 요약 값이다.

존재 이유: 검사한 상태와 실행할 상태가 다른지 확인한다.

구체 예: 계획 이후 source snapshot이 바뀌면 원래 지문과 달라진다.

혼동 주의: 지문이 같다는 사실만으로 다른 프로세스가 없다는 보장이 생기지 않는다.
</term>

<term id="operation" title="작업 기록(operation)" aliases="operationId|영속 작업 기록">
작업 기록은 한 번의 전환이 어디까지 진행되었는지 저장한 자료다.

존재 이유: 실패나 앱 종료 뒤에 같은 전환을 판별한다.

구체 예: 원격 Space는 만들었지만 태스크 연결은 아직 변경하지 않은 상태다.

혼동 주의: 완료된 작업 기록은 명령의 외부 부수효과를 취소하는 자료가 아니다.
</term>

<term id="cas" title="조건부 갱신(compare-and-set)" aliases="compare-and-set">
조건부 갱신은 현재 값이 예상 값과 같을 때만 저장 값을 바꾸는 동작이다.

존재 이유: 다른 창이 변경한 태스크 연결을 덮어쓰지 않는다.

구체 예: 이전 sessionName이 예상 값일 때만 새 sessionName을 저장한다.
</term>

<term id="ssh" title="SSH(Secure Shell)" aliases="SSH">
SSH는 다른 호스트에서 인증된 명령과 통신을 실행하는 연결 방식이다.

존재 이유: B가 A의 Herdr 서버와 통신한다.

구체 예: B에 등록한 `dev-vm` 별칭이다.

혼동 주의: SSH 접속 가능과 Herdr 기능 호환성은 별도 조건이다.
</term>

<term id="handoff" title="live handoff" aliases="live handoff">
live handoff는 지원하는 Herdr 서버를 교체할 때 살아 있는 실행 상태를 넘기는 실험 기능이다.

존재 이유: 특정 서버 교체에서 프로세스 중단을 줄인다.

구체 예: 지원되는 업데이트에서 명시적으로 선택하는 handoff다.

혼동 주의: 독립 세션 병합을 보장하는 기능은 아니다.
</term>


<term id="client" title="클라이언트(client)" aliases="client">
클라이언트는 사용자의 입력을 서버에 보내고 화면을 표시하는 프로그램이다.

존재 이유: 화면을 그리는 기기와 작업을 실행하는 기기를 분리한다.

구체 예: B에서 실행하는 Herdr client다.

혼동 주의: 클라이언트를 바꾸어도 원본 서버가 이동하지 않는다.
</term>

<term id="server" title="서버(server)" aliases="서버">
서버는 클라이언트의 요청을 받아 작업 상태를 관리하는 실행 주체다.

존재 이유: 사용자가 화면을 닫아도 작업을 계속 관리한다.

구체 예: A의 kanvibe Herdr 서버다.

혼동 주의: 이 문서의 Electron main 서비스와 원격 Herdr 서버는 서로 다르다.
</term>

<term id="pid" title="프로세스 식별자(Process ID, PID)" aliases="PID">
PID는 운영체제가 실행 중인 프로세스에 붙이는 식별자다.

존재 이유: 연결 전후에 같은 실행이 유지되는지 확인한다.

구체 예: 원본 테스트 프로그램의 PID다.

혼동 주의: 프로그램을 재시작하면 같은 대화를 열어도 같은 PID를 보장하지 않는다.
</term>

<term id="pty" title="가상 터미널(pseudoterminal, PTY)" aliases="PTY">
PTY는 프로그램에 터미널 입력과 출력을 제공하는 운영체제 장치다.

존재 이유: 셸과 에이전트가 대화형 프로그램으로 동작한다.

구체 예: Herdr pane이 사용하는 터미널이다.

혼동 주의: PTY 소유권 이전은 화면을 다시 그리는 것보다 큰 작업이다.
</term>

<term id="scrollback" title="스크롤백(scrollback)" aliases="스크롤백">
스크롤백은 터미널이 이전에 출력한 화면 기록이다.

존재 이유: 화면 위로 스크롤해 지난 출력을 읽는다.

구체 예: 오래 전에 끝난 테스트의 출력이다.

혼동 주의: 배치와 대화 기록을 복원해도 같은 스크롤백이 생기지는 않는다.
</term>

<term id="sqlite" title="SQLite" aliases="SQLite">
SQLite는 앱이 로컬 파일에 구조화된 데이터를 저장하는 데이터베이스다.

존재 이유: KanVibe가 태스크의 현재 연결을 유지한다.

구체 예: kanban_tasks에 저장된 sessionName이다.

혼동 주의: 로컬 SQLite 쓰기는 원격 Herdr 변경과 하나의 원자적 작업이 아니다.
</term>

<term id="renderer" title="렌더러(renderer)" aliases="renderer">
렌더러는 Electron에서 사용자가 보는 화면을 실행하는 프로세스다.

존재 이유: 설정과 태스크 화면을 표시한다.

구체 예: SettingsRoute가 실행되는 쪽이다.

혼동 주의: 시스템 명령은 renderer에서 직접 실행하지 않는다.
</term>

<term id="main" title="Electron main" aliases="Electron main">
Electron main은 데스크톱 앱의 시스템 기능과 창을 관리하는 프로세스다.

존재 이유: 파일과 SSH와 데이터베이스 기능을 화면과 분리한다.

구체 예: kanvibe:invoke를 처리하는 프로세스다.

혼동 주의: 원격 A의 Herdr 서버와 같은 프로세스가 아니다.
</term>

<term id="decoder" title="해석기(decoder)" aliases="decoder">
해석기는 저장된 데이터를 지원하는 구조로 읽는 코드다.

존재 이유: 과거 snapshot의 형식 차이를 판별한다.

구체 예: 지원하지 않는 session.json을 재구성 전에 거부하는 코드다.

혼동 주의: 파싱에 성공했다고 모든 상태를 복원할 수 있는 것은 아니다.
</term>

<term id="adapter" title="어댑터(adapter)" aliases="adapter">
어댑터는 특정 도구의 입력과 출력을 앱의 계약에 맞추는 코드다.

존재 이유: 제공자별 대화 참조와 재개 명령의 차이를 분리한다.

구체 예: Codex와 Claude의 서로 다른 재개 인자를 처리한다.

혼동 주의: 새로운 외부 기능을 어댑터만으로 만들어 낼 수는 없다.
</term>

<term id="endpoint" title="연결 대상(endpoint)" aliases="endpoint">
연결 대상은 클라이언트가 통신하는 서버 쪽 끝이다.

존재 이유: 서로 다른 호스트와 서버의 요청을 구분한다.

구체 예: B에서 선택하는 A의 kanvibe 연결이다.

혼동 주의: 다른 endpoint에 같은 w1 ID가 있을 수 있다.
</term>

<term id="worktree" title="작업 디렉터리(Git worktree)" aliases="worktree">
작업 디렉터리는 특정 Git 작업의 파일을 사용하는 디렉터리다.

존재 이유: 이전 전후에 같은 파일과 미커밋 변경을 사용한다.

구체 예: 사용자가 기존 프로젝트를 수정하던 경로다.

혼동 주의: 화면을 재구성하는 일과 파일을 복사하는 일은 다르다.
</term>

<term id="transaction" title="트랜잭션(transaction)" aliases="트랜잭션">
트랜잭션은 묶인 저장 작업이 함께 성공하거나 실패하도록 하는 단위다.

존재 이유: 연결 필드와 로컬 기록의 일부 불일치를 막는다.

구체 예: 로컬 DB의 조건부 연결 갱신이다.

혼동 주의: 원격 Space 생성까지 자동으로 같은 트랜잭션에 포함되지는 않는다.
</term>

<term id="uuid" title="UUID(Universally Unique Identifier)" aliases="UUID">
UUID는 작업과 데이터를 구별하는 식별자 형식이다.

존재 이유: 재요청이 같은 전환을 가리키는지 확인한다.

구체 예: operationId로 사용하는 문자열이다.

혼동 주의: 새 UUID로 재시도하면 기존 요청의 중복으로 판별하지 못할 수 있다.
</term>

<term id="sha256" title="SHA-256" aliases="SHA-256">
SHA-256은 입력 데이터에서 고정 길이의 해시 값을 계산하는 함수다.

존재 이유: 원본 자료의 변경을 지문으로 비교한다.

구체 예: 검사한 snapshot과 현재 snapshot의 해시다.

혼동 주의: 해시 비교는 실행 상태 재검사를 대체하지 않는다.
</term>

<term id="namespace" title="네임스페이스(namespace)" aliases="namespace">
네임스페이스는 호출할 서비스 이름을 구분하는 범위다.

존재 이유: IPC 요청이 어느 서비스의 함수인지 선택한다.

구체 예: 제안한 herdrSession이다.

혼동 주의: 현재 구현에 이미 등록된 서비스 이름이라고 단정하지 않는다.
</term>

<term id="flag" title="기능 플래그(feature flag)" aliases="feature flag">
기능 플래그는 앱 기능의 사용 가능 여부를 제어하는 설정이다.

존재 이유: 읽기 검사부터 순서대로 기능을 공개한다.

구체 예: 쓰기 기능을 중단하고 기존 연결을 유지하는 설정이다.

혼동 주의: 문서의 제안은 이미 배포된 기능 설정이 아니다.
</term>

<term id="native-restore" title="native agent restore" aliases="native agent restore">
native agent restore는 저장한 에이전트 참조로 지원되는 대화를 새 실행에서 여는 복원 방식이다.

존재 이유: 서버 재시작 뒤에도 지원되는 대화를 이어간다.

구체 예: 유효한 제공자 세션 ID로 도구를 다시 여는 경우다.

혼동 주의: 일반 프로세스나 실행 중인 명령의 메모리를 복원하지 않는다.
</term>

## 부록: 근거와 확인 범위 {#sources toc="부록: 근거"}

### 공식 기능 근거 {#official-sources toc="공식 기능 근거"}

- [Herdr 0.9.5 Connecting machines](https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/connecting-machines.mdx): 프로필의 대상, 호스트별 실행 소유권, 연결 실패 처리다.
- [Herdr 0.9.5 Session state](https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/session-state.mdx): 재접속, 재시작, 대화 재개와 handoff의 보존 범위다.
- [Herdr 0.9.5 CLI reference](https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/cli-reference.mdx): session, workspace, pane, machine 명령이다.
- [Herdr 0.9.5 client 구현](https://github.com/herdrdev/herdr/blob/v0.9.5/src/client/mod.rs): endpoint catalog와 client별 연결 처리다.
- [Tailscale Serve](https://tailscale.com/docs/reference/tailscale-cli/serve): 이 설명 페이지를 tailnet 내부에 노출하는 도구다.

### 확인한 KanVibe 코드 {#code-sources toc="확인한 KanVibe 코드"}

원격 GitHub 링크는 조사한 기준 커밋 `20af2db9`다.
로컬 작업 트리의 구현을 함께 읽었다.
앞서 수정한 이미지 붙여넣기 변경은 이번 세션 이전 기능의 구현 증거가 아니다.

- [Herdr 명령과 작업 키 매핑](https://github.com/rookedsysc/kanvibe/blob/20af2db9/src/lib/herdr.ts)
- [PTY와 SSH 연결](https://github.com/rookedsysc/kanvibe/blob/20af2db9/src/lib/terminal.ts)
- [client별 Space 선택](https://github.com/rookedsysc/kanvibe/blob/20af2db9/src/lib/herdrClientProxy.ts)
- [기존 branch 세션 재사용](https://github.com/rookedsysc/kanvibe/blob/20af2db9/src/desktop/main/services/projectService.ts)
- [현재 태스크 저장 필드](https://github.com/rookedsysc/kanvibe/blob/20af2db9/src/entities/KanbanTask.ts)
- [IPC namespace 등록](https://github.com/rookedsysc/kanvibe/blob/20af2db9/src/desktop/main/serviceRegistry.ts)

### 사용자가 제공한 근거 {#user-sources toc="사용자가 제공한 근거"}

사용자는 맥북의 세션 목록과 Herdr 화면 두 장을 제공했다.
목록은 기존 독립 세션과 공유 kanvibe 서버가 함께 존재함을 보여준다.
사용자는 B에서 A의 기존 작업을 보고 싶다고 요청했다.
이 자료만으로 맥북의 실제 snapshot 형식과 현재 PID를 확인할 수는 없다.
