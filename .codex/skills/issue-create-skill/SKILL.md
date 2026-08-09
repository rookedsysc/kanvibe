---
name: issue-create-skill
description: "GitHub Issue 생성 스킬. gh CLI로 GitHub 레포지토리에 이슈를 생성한다. ARGUMENTS로 GitHub repo URL과 이슈 내용(제목, 본문, 라벨 등)을 받는다. 트리거 키워드: issue, 이슈, 이슈생성, create issue, gh issue, 버그리포트, feature request."
---

# GitHub Issue Create

`gh issue create`를 사용하여 GitHub 레포지토리에 이슈를 생성한다.

## Arguments 형식

```
<github-repo-url> <issue-content>
```

- `github-repo-url`: GitHub 레포지토리 URL (예: `https://github.com/owner/repo`)
- `issue-content`: 이슈 제목과 본문을 포함한 자유 형식 텍스트

### 예시

```
https://github.com/owner/repo 로그인 시 500 에러 발생. 재현 방법: 1. 로그인 페이지 접속 2. 이메일/비밀번호 입력 3. 로그인 버튼 클릭 시 500 에러
```

## 워크플로우

### 1. Arguments 파싱

ARGUMENTS에서 GitHub repo URL과 이슈 내용을 분리한다.

- 첫 번째 `https://github.com/...` 패턴을 repo URL로 추출
- 나머지 텍스트를 이슈 내용으로 사용

### 2. 이슈 내용 구조화

사용자가 제공한 내용을 분석하여 제목과 본문을 구성한다.

**제목 생성 규칙:**
- 사용자가 명시적으로 제목을 지정한 경우 그대로 사용
- 지정하지 않은 경우, 내용을 요약하여 간결한 제목 생성 (50자 이내)

**본문 구성:**
- 사용자가 제공한 내용을 정리하여 마크다운으로 작성
- 내용에 따라 적절한 섹션 구분 (예: 설명, 재현 방법, 기대 동작 등)

### 3. 이슈 생성

```bash
gh issue create --repo <owner/repo> --title "<title>" --body "$(cat <<'EOF'
<body>
EOF
)"
```

## 주의사항

- `gh auth status`로 인증 상태를 먼저 확인
- repo URL에서 `owner/repo` 형식을 추출하여 `--repo` 옵션에 전달
- 본문이 길 경우 HEREDOC 사용
- 이슈 생성 후 생성된 이슈 URL을 사용자에게 반환
