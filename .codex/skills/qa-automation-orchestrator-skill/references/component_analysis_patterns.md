# 컴포넌트 분석 패턴

컴포넌트를 자동으로 분석하고 우선순위를 산정하는 패턴들입니다.

## 컴포넌트 타입 분류

### 1. UI 컴포넌트
- Button, Input, Form 등
- 테스트 전략: 상호작용, 상태, 에러 처리
- 우선순위: 비즈니스 임팩트에 따라

### 2. 컨테이너 컴포넌트
- Page, Layout, Modal 등
- 테스트 전략: 렌더링, 네비게이션, 데이터 흐름
- 우선순위: HIGH

### 3. 비즈니스 컴포넌트
- LoginForm, CheckoutForm 등
- 테스트 전략: 종합 테스트, 에러 시나리오
- 우선순위: CRITICAL

## 우선순위 산정 기준

### CRITICAL
- 사용자 인증 관련
- 결제/거래 관련
- 데이터 손실 위험

### HIGH
- 주요 사용자 플로우
- 폼 입력/검증
- 네비게이션

### MEDIUM
- 일반 컴포넌트
- 표시 기능
- 보조 기능

### LOW
- 유틸리티 함수
- 헬퍼 함수
- 상수

## 의존성 분석

```
LoginComponent
  ├── AuthService (CRITICAL)
  ├── InputComponent (HIGH)
  └── ErrorMessage (MEDIUM)

CheckoutComponent
  ├── PaymentService (CRITICAL)
  ├── FormComponent (HIGH)
  ├── PriceCalculator (MEDIUM)
  └── CartItem (HIGH)
```

## 테스트 케이스 자동 생성

각 컴포넌트 타입별 자동 생성되는 테스트:

### UI 컴포넌트
1. 렌더링 테스트
2. Props 전달 테스트
3. 이벤트 핸들러 테스트
4. 상태 변화 테스트

### 페이지 컴포넌트
1. 로드 테스트
2. 네비게이션 테스트
3. 쿼리 파라미터 테스트
4. 반응형 디자인 테스트

### 서비스
1. API 호출 테스트
2. 데이터 변환 테스트
3. 에러 처리 테스트
4. 캐싱 테스트
