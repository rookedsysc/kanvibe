---
name: technical-search-skill
description: Research technical topics and write comprehensive blog posts in Roky's distinctive Korean style. Use when the user requests (1) Creating technical blog posts about programming concepts frameworks or technologies (2) Researching and documenting technical topics with detailed explanations (3) Writing posts that combine deep technical analysis with personal learning experiences (4) Creating study materials or documentation in a friendly conversational Korean style. Triggers include 블로그 포스팅 기술글 작성 조사 research write-about or requests to explain technical concepts in blog format with expressions like 에대해글써줘 를조사해줘 or file save requests with @파일명 or 파일명에저장넣어작성
---

# Technical Search & Roky-Style Blog Writing Skill

기술 내용을 검색하고 Roky 스타일의 블로그 포스팅을 작성하는 통합 워크플로우입니다.

## Workflow Overview

이 스킬은 두 단계로 동작합니다:

1. **검색 단계**: references/search-methodology.md의 가이드에 따라 기술 정보 수집
2. **작성 단계**: references/roky-writing-style.md의 스타일로 블로그 포스팅 작성

## Step 1: 입력 파싱 및 검색 준비

사용자 입력에서 다음 정보를 추출:

### 검색 키워드 추출
사용자가 조사하고자 하는 기술 주제 파악

### 파일 저장 여부 확인
- **명시적 지정**: `@파일명` 또는 `파일명에 {저장/넣어/작성}줘`
- **미지정 시**: 검색 키워드/주제를 기반으로 자동 파일명 생성
  - 형식: `{주제}-{세부내용}.mdx`
  - 예시: `react-useEffect정리함수.mdx`, `typescript-제네릭-활용법.mdx`
  - 한글/이모지 포함으로 Bash heredoc 사용 필수

### Arguments 확인
- `--c7`: Context7 우선 사용 지시

## Step 2: 기술 내용 검색

**중요**: references/search-methodology.md를 참조하여 검색 수행

### 검색 우선순위 적용

1. **인터넷 검색 (WebSearch)**
   - 잘 알려지지 않은 내용이나 최신 정보
   - 반드시 Sources 섹션 포함

2. **Context7 (--c7 플래그 시 또는 공식 문서 필요 시)**
   - mcp__context7__resolve-library-id로 라이브러리 ID 확보
   - mcp__context7__query-docs로 문서 검색
   - **최대 3회 호출 제한**
   - 민감 정보 절대 포함 금지

### 검색 결과 구조화

수집한 정보를 다음 순서로 정리:
1. 결론 우선 - 핵심 답변
2. 세부 내용 - 구체적 설명
3. 코드 예시 - 필요한 경우만
4. 대안 제시 - 있는 경우

### 필수 검증 체크리스트

- [ ] 기술적으로 정확한가?
- [ ] 최신 버전 정보인가?
- [ ] 공식 문서와 일치하는가?
- [ ] 질문자의 의도에 적합한가?
- [ ] 불필요한 내용은 제외했는가?
- [ ] 출처가 모두 표기되어 있는가?

## Step 3: Roky 스타일 블로그 작성

**중요**: references/roky-writing-style.md를 참조하여 작성

### 말투 및 톤
- 한국어 반말 사용 (~이다, ~했다, ~보자)
- 직접적이고 단도직입적
- 친근하면서 전문적
- 솔직한 학습 과정 공유
- 겸손한 태도

### 글 구조

#### 1. 도입부
개인적 경험이나 학습 계기로 시작

#### 2. 본문

**원리 설명**
- 기본 개념부터 차근차근
- "~를 살펴보자", "~에 대해 알아보자"

**문제 상황** (해당되는 경우)
- ⚠️ 이모지로 강조
- 구체적인 시나리오나 코드로 재현

**해결 방법**
- 🛠️ 이모지로 섹션 구분
- 여러 해결책이 있다면 모두 제시
- **완전한 코드 제공** (부분이 아닌 전체)
- 한국어 주석 포함
- 중요한 차이점은 "★" 주석으로 강조

**비교 및 분석**
- 장점/단점 명확히 구분
- 구체적인 수치와 예시
- 표, 로그, 스크린샷 활용

**에러 경험** (해당되는 경우)
- 에러 메시지 전체 포함
- 원인 분석
- 해결 과정 단계별 서술

#### 3. 검증 및 테스트
- 실제 테스트 결과 제시
- 구체적인 도구 언급 (K6, JVM 프로파일링 등)
- 로그 포함
- 비교 표나 그래프

#### 4. 마무리: "마치며"
학습한 내용 요약 또는 소감, 겸손한 태도로 추가 조언 요청

### 이모지 사용
- ⚠️: 문제 상황, 주의사항
- 🛠️: 해결 방법, 구현
- ✅: 성공, 허용
- ❌: 실패, 거부
- 📊: 테스트 결과, 벤치마크

### 코드 작성
- 전체 코드 제공 (일부 생략 금지)
- 한국어 주석 포함
- 출처가 있다면 코드 블록 하단에 명시

### 출처 표기 필수
각 내용별로 반드시 출처 링크와 원문 인용 포함

## Step 4: 파일 저장

### 저장 위치 결정
- 사용자 지정 파일명이 있으면 사용
- 없으면 자동 생성: `{주제}-{세부내용}.mdx`

### 저장 방법
한글/이모지 포함으로 Bash heredoc 사용 필수

### 기존 파일 처리
- 기존 파일이 존재하면 Read 후 처리 방향 결정
- 덮어쓰기 또는 병합 선택

## References

작업 중 필요에 따라 다음 파일들을 참조:

- **references/search-methodology.md**: 검색 방법론 상세 가이드 (검색 도구 사용법, 우선순위, 출처 표기)
- **references/roky-writing-style.md**: Roky 스타일 작성법 상세 가이드 (말투, 구조, 이모지, 코드 작성법)

## Example Usage

**사용자**: "React useEffect의 cleanup function에 대해 블로그 글 써줘"

**실행 흐름**:
1. 검색 키워드 추출: "React useEffect cleanup function"
2. WebSearch 및/또는 Context7로 정보 수집
3. 출처와 함께 정보 정리
4. Roky 스타일로 블로그 작성
5. `react-useEffect-cleanup.mdx`로 자동 저장

**사용자**: "Spring Boot의 트랜잭션 전파에 대해 조사해서 @트랜잭션전파.mdx에 저장해줘 --c7"

**실행 흐름**:
1. 검색 키워드: "Spring Boot transaction propagation"
2. --c7 플래그로 Context7 우선 사용
3. Spring Boot 공식 문서에서 정보 수집
4. Roky 스타일로 작성
5. 지정된 파일명 `트랜잭션전파.mdx`로 저장

## Notes

- 반드시 references 파일들을 참조하여 각 단계의 세부 사항 확인
- 출처 없는 정보는 작성하지 않음
- Context7은 최대 3회만 호출
- 한글 파일명 사용 시 Bash heredoc 필수
- 블로그 게재 수준의 자연스러운 문장 작성
