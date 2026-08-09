# 번역 용어 사전

공식 문서 번역 시 일관성을 위한 용어 가이드입니다.

## 일반 용어

| English | 한국어 | 비고 |
|---------|--------|------|
| Getting Started | 시작하기 | |
| Quick Start | 빠른 시작 | |
| Introduction | 소개 | |
| Overview | 개요 | |
| Installation | 설치 | |
| Setup | 설정 | |
| Configuration | 구성, 설정 | 맥락에 따라 선택 |
| Guide | 가이드 | |
| Tutorial | 튜토리얼 | |
| Documentation | 문서 | |
| Reference | 레퍼런스 | |
| API Reference | API 레퍼런스 | API는 번역 안 함 |
| Examples | 예제 | |
| Advanced | 고급 | |
| Basic | 기초 | |
| Features | 기능 | |
| Usage | 사용법 | |
| Best Practices | 모범 사례 | |
| Tips | 팁 | |
| Troubleshooting | 문제 해결 | |
| FAQ | 자주 묻는 질문 | |
| Changelog | 변경 로그 | |
| Migration Guide | 마이그레이션 가이드 | |

## 개발 용어

| English | 한국어 | 비고 |
|---------|--------|------|
| Component | 컴포넌트 | |
| Props | Props | 번역 안 함 |
| State | State | 번역 안 함 (문맥상 "상태"도 가능) |
| Hook | Hook | 번역 안 함 |
| Render | 렌더링하다 | |
| Build | 빌드하다 | |
| Deploy | 배포하다 | |
| Compile | 컴파일하다 | |
| Bundle | 번들링하다 | |
| Import | Import하다 | |
| Export | Export하다 | |
| Function | 함수 | |
| Class | 클래스 | |
| Method | 메서드 | |
| Property | 프로퍼티, 속성 | |
| Parameter | 파라미터, 매개변수 | |
| Argument | 인자 | |
| Return | 반환하다 | |
| Callback | 콜백 | |
| Event | 이벤트 | |
| Handler | 핸들러 | |
| Listener | 리스너 | |
| Middleware | 미들웨어 | |
| Plugin | 플러그인 | |
| Extension | 확장 | |
| Module | 모듈 | |
| Package | 패키지 | |
| Library | 라이브러리 | |
| Framework | 프레임워크 | |
| Repository | 레포지토리 | |
| Branch | 브랜치 | |
| Commit | 커밋 | |
| Pull Request | Pull Request, PR | |
| Issue | Issue | |
| Bug | 버그 | |
| Feature | 기능 | |
| Performance | 성능 | |
| Optimization | 최적화 | |
| Cache | 캐시 | |
| Database | 데이터베이스 | |
| Query | 쿼리 | |
| Schema | 스키마 | |
| Model | 모델 | |
| Controller | 컨트롤러 | |
| View | 뷰 | |
| Route | 라우트 | |
| Endpoint | 엔드포인트 | |
| Request | 요청 | |
| Response | 응답 | |
| Authentication | 인증 | |
| Authorization | 권한 부여 | |
| Validation | 유효성 검사 | |
| Error | 에러 | |
| Exception | 예외 | |
| Debug | 디버그하다 | |
| Test | 테스트 | |
| Unit Test | 유닛 테스트 | |
| Integration Test | 통합 테스트 | |

## UI/UX 용어

| English | 한국어 | 비고 |
|---------|--------|------|
| Button | 버튼 | |
| Input | 입력, 입력 필드 | |
| Form | 폼, 양식 | |
| Modal | 모달 | |
| Dialog | 다이얼로그 | |
| Dropdown | 드롭다운 | |
| Menu | 메뉴 | |
| Navigation | 네비게이션 | |
| Sidebar | 사이드바 | |
| Header | 헤더 | |
| Footer | 푸터 | |
| Layout | 레이아웃 | |
| Grid | 그리드 | |
| Container | 컨테이너 | |
| Wrapper | 래퍼 | |
| Theme | 테마 | |
| Style | 스타일 | |
| CSS | CSS | 번역 안 함 |
| Responsive | 반응형 | |
| Mobile | 모바일 | |
| Desktop | 데스크톱 | |
| Tablet | 태블릿 | |
| Accessibility | 접근성 | |
| Animation | 애니메이션 | |
| Transition | 전환 | |

## 번역 규칙

### 1. 프레임워크/라이브러리 이름
항상 영문 그대로 사용
- React, Vue, Angular, Next.js, Nuxt.js, etc.

### 2. 함수/변수명
코드 내의 이름은 번역하지 않음
```jsx
// 올바른 예
`useState` Hook은 상태를 관리하는 데 사용됩니다.

// 잘못된 예
`상태사용` Hook은 상태를 관리하는 데 사용됩니다.
```

### 3. 기술 용어 처음 사용 시
괄호 안에 원어 또는 설명 추가 가능
```
서버 측 렌더링(Server-Side Rendering, SSR)은...
```

### 4. 존댓말 사용
공식 문서는 존댓말(합니다/습니다) 사용
```
이 기능을 사용하면 성능을 향상시킬 수 있습니다.
```

### 5. 코드 블록
절대 번역하지 않음. 주석도 가급적 원문 유지

### 6. URL 및 링크
변경하지 않음

### 7. 마크다운 구조
원본 구조 유지 (헤딩 레벨, 리스트 형식 등)

## 자주 틀리는 표현

| 잘못된 표현 | 올바른 표현 |
|------------|------------|
| 리엑트 | React |
| 뷰 | Vue |
| 넥스트제이에스 | Next.js |
| 컴포넌트를 렌더한다 | 컴포넌트를 렌더링한다 |
| State 값 | State (또는 상태 값) |
| Props를 전달한다 | Props를 전달한다 ✓ |
| API를 호출한다 | API를 호출한다 ✓ |
| 함수를 콜한다 | 함수를 호출한다 |
