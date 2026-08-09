# 기술 내용 검색 방법론

이 문서는 기술 내용을 효과적으로 검색하고 정보를 수집하는 방법을 설명합니다.

## 검색 우선순위

### 1. 인터넷 검색 우선 (WebSearch)
- **사용 시점**: 잘 알려지지 않은 내용이나 최신 정보
- **도구**: WebSearch
- **예시**: 최신 프레임워크 버전, 최근 발생한 이슈, 트렌드

### 2. Context7 활용
- **사용 시점**: 공식 문서, 라이브러리 사용법, 프레임워크 패턴
- **도구**: mcp__context7__resolve-library-id → mcp__context7__query-docs
- **중요**: 최대 3회 호출 제한
- **프로세스**:
  1. resolve-library-id로 라이브러리 ID 확보
  2. query-docs로 구체적 질문 수행
  3. 민감 정보(API 키, 패스워드) 절대 포함 금지

### 3. 출처 표기 필수
모든 검색 결과는 반드시 출처와 함께 원문 인용을 포함해야 합니다.

## 정보 수집 원칙

### 검색어 작성
- **구체적으로**: "auth" ❌ → "JWT authentication in Express.js" ✅
- **최신 연도 명시**: "React documentation" ❌ → "React documentation 2026" ✅
- **컨텍스트 포함**: "hooks" ❌ → "React useEffect cleanup function examples" ✅

### 정보 검증
답변 작성 전 필수 체크:
- [ ] 기술적으로 정확한가?
- [ ] 최신 버전 정보인가?
- [ ] 공식 문서와 일치하는가?
- [ ] 질문자의 의도에 적합한가?
- [ ] 불필요한 내용은 제외했는가?

## 출처 표기 형식

```markdown
## 결론
- [핵심 내용 1] [출처명](https://example.com/path1)

> 위 출처에서 "실제 원문 내용 인용" 이라고 명시되어 있음.

- [핵심 내용 2] [출처명](https://example.com/path2)

> 위 출처에서 "실제 원문 내용 인용" 이라고 명시되어 있음.
```

## Context7 사용 예시

```markdown
1. 라이브러리 ID 확보:
   mcp__context7__resolve-library-id
   - libraryName: "react"
   - query: "React useEffect cleanup function 사용법"

2. 문서 검색:
   mcp__context7__query-docs
   - libraryId: "/facebook/react" (1단계에서 획득)
   - query: "How to properly cleanup effects in useEffect hook"

3. 최대 3회까지만 호출 - 이후는 수집한 정보로 작성
```

## 검색 결과 구조화

수집한 정보는 다음 순서로 정리:

1. **결론 우선** - 핵심 답변 최상단 배치
2. **세부 내용** - 결론을 뒷받침하는 구체적 설명
3. **코드 예시** - 필요한 경우만, 간단하게
4. **대안 제시** - 있는 경우 1-2줄 요약

## 주의사항

- WebSearch 사용 시 반드시 Sources 섹션 포함
- Context7은 민감 정보 절대 포함 금지
- 호출 횟수 제한 준수 (Context7: 3회)
- 출처 없는 정보는 작성하지 않음
