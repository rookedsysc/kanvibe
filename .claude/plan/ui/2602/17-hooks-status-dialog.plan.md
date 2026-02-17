# 플랜: Task 상세 페이지 Hooks Status Dialog 구현

## 비즈니스 목표

Task 상세 페이지에서 Hooks 상태를 Dialog로 표시하여 사용자 경험을 개선하고, ProjectSettings 목록에는 신호등 표시기만 노출하여 UI 복잡도를 낮춘다.

## 구현 세부사항

### 대상 파일

1. **src/components/ProjectSettings.tsx** - 신호등 표시기 추가
2. **src/app/[locale]/task/[id]/page.tsx** - Dialog 오픈 버튼 추가
3. **src/components/HooksStatusDialog.tsx** - 새 Dialog 컴포넌트 (생성)
4. **src/hooks/useHooksStatus.ts** - Custom hook (생성, 선택사항)

### 신호등 상태 로직

```typescript
// 상태 계산 로직
- 모두 설치 (Claude, Gemini, Codex 모두 Installed) → 🟢 "All OK"
- 일부 설치 → 🟡 "Partial"
- 미설치 (모두 미설치) → 🔴 "Not Installed"
```

### HooksStatusDialog 구조

```tsx
interface HooksStatusDialogProps {
  isOpen: boolean;
  onClose: () => void;
  projectId: string;
  isPending?: boolean;
}

// 렌더링
<Dialog>
  <h2>Hooks Status</h2>

  {/* Claude Hooks */}
  <div>
    <span>Claude</span>
    <span>{status.installed ? "Installed" : "Not Installed"}</span>
    <button onClick={handleInstall}>Install Hooks</button> or <button>Reinstall</button>
  </div>

  {/* Gemini Hooks */}
  {/* Codex Hooks */}

  <button onClick={onClose}>Close</button>
</Dialog>
```

## 컨벤션 준수

- **명명 규칙**: `HooksStatusDialog` (기존 Dialog 패턴 따름)
- **상태 관리**: useState + useTransition (기존 패턴)
- **i18n**: useTranslations("task") 또는 새 namespace
- **스타일**: Tailwind CSS v4 + 기존 디자인 토큰 사용
- **Custom Hook**: useHooksStatus (선택사항 - 상태 로직 분리 시)

## 단계별 Todos

### Todo 1: useHooksStatus Custom Hook 생성 (우선도 1, 선택사항)
- **목표**: 신호등 상태 계산 로직을 재사용 가능한 hook으로 분리
- **상세 작업**:
  - `src/hooks/useHooksStatus.ts` 생성
  - projectId → hooks status 조회 로직 구현
  - 상태 계산: (installed count) 기반 신호등 결정
  - Return type: `{ claudeStatus, geminiStatus, codexStatus, overallStatus }`
- **검증 기준**: 모든 조합의 상태 계산 정확도 확인
- **종료 기준**: Hook이 정상 작동하고 재사용 가능

### Todo 2: HooksStatusDialog 컴포넌트 생성 (우선도 1)
- **목표**: Hooks 상태를 표시하는 Dialog 컴포넌트 구현
- **상세 작업**:
  - `src/components/HooksStatusDialog.tsx` 생성
  - Props: isOpen, onClose, projectId, isPending
  - 내부 로직:
    - useTransition으로 설치 상태 관리
    - getProjectHooksStatus, getProjectGeminiHooksStatus, getProjectCodexHooksStatus 호출
    - 각 hooks별 설치/재설치 버튼 렌더링
  - 스타일: 기존 Dialog 패턴 (DoneConfirmDialog 참고)
- **검증 기준**:
  - Dialog 열기/닫기 정상 작동
  - 각 hooks 상태 표시 정확
  - 설치 버튼 클릭 시 action 실행
- **종료 기준**: Dialog 렌더링 및 상호작용 완벽

### Todo 3: Task 상세 페이지에서 Dialog 오픈 버튼 추가 (우선도 2, Todo 2 의존)
- **목표**: Task 상세 페이지 기존 "Hooks Status" 섹션을 Dialog 오픈 버튼으로 대체
- **상세 작업**:
  - `src/app/[locale]/task/[id]/page.tsx` 수정
  - 기존 hooks status 섹션 제거
  - Dialog 오픈 버튼 추가 (신호등 + 텍스트)
  - HooksStatusDialog import 및 state 관리
  - 신호등 상태 계산 (useHooksStatus 또는 inline)
- **검증 기준**:
  - 버튼 클릭 시 Dialog 열림
  - 신호등 표시 정확
- **종료 기준**: Task 상세 페이지에서 Dialog 정상 작동

### Todo 4: ProjectSettings 목록에 신호등 추가 (우선도 2)
- **목표**: 각 프로젝트의 신호등 상태 표시기 추가
- **상세 작업**:
  - `src/components/ProjectSettings.tsx` 수정
  - 프로젝트별 hooks 상태 계산
  - Delete 버튼 우측에 신호등 아이콘 + 상태명 표시
  - 신호등 색상: 🟢 (All OK) / 🟡 (Partial) / 🔴 (Not Installed)
- **검증 기준**:
  - 신호등 표시 정확도
  - UI 레이아웃 깔끔함
- **종료 기준**: 모든 프로젝트에 신호등 표시됨

### Todo 5: i18n 번역 추가 (우선도 3, Todo 2-3 의존)
- **목표**: Dialog 및 상태 텍스트 다국어 지원
- **상세 작업**:
  - `messages/ko.json`, `messages/en.json`, `messages/zh.json`에 필요한 키 추가
  - Keys: "hooksStatus", "hooksInstalled", "notInstalled", "allOk", "partial", etc.
- **검증 기준**: 모든 언어 번역 완료 및 정확도
- **종료 기준**: 다국어 지원 정상 작동

### Todo 6: 스타일링 및 디자인 토큰 적용 (우선도 3, Todo 2-4 의존)
- **목표**: Dialog 및 신호등 UI를 디자인 시스템에 맞게 구성
- **상세 작업**:
  - Dialog: 기존 패턴 (bg-bg-surface, border-border-default, etc.) 적용
  - 신호등: 상태별 색상 토큰 사용
    - All OK: `text-status-done` or `text-brand-primary`
    - Partial: `text-status-review` or `text-yellow-500`
    - Not Installed: `text-status-error`
- **검증 기준**: 디자인 일관성
- **종료 기준**: UI가 디자인 시스템과 일치

## 검증 전략

### 스태틱 검증
1. TypeScript 컴파일 성공 (`pnpm check`)
2. ESLint 통과 (`pnpm lint`)

### 동적 검증
1. 개발 서버 시작 (`pnpm dev`)
2. Task 상세 페이지에서:
   - Dialog 오픈 버튼 표시 ✓
   - 신호등 표시 (상태별 정확도) ✓
   - Dialog 열기 ✓
   - Dialog에서 hooks 상태 표시 ✓
   - 설치/재설치 버튼 작동 ✓
3. ProjectSettings 목록에서:
   - 신호등 표시 ✓
   - 상태명 표시 ✓

## 컨벤션 준수 노트

- **KISS**: 필요한 최소 기능만 구현
- **네이밍**: 기존 Dialog 컴포넌트 패턴 따름 (DoneConfirmDialog, CreateTaskModal 참고)
- **스타일**: Tailwind CSS v4 + 디자인 시스템 토큰 사용
- **상태 관리**: useState + useTransition (기존 패턴)
- **다국어**: next-intl 활용

## 오픈 질문

없음 - 요구사항 명확함
