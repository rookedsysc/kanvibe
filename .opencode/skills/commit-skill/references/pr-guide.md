# PR 생성 및 관리 가이드

**--only-commit 플래그가 없을 때 이 가이드를 따른다.**

---

## AI 작성자 표기

기본 동작에서는 PR 제목과 PR 본문에 AI 작성자 표기를 추가하지 않는다.

`--aiauthor` 플래그가 있으면 PR 본문 마지막에만 AI 작성자 표기를 추가한다.

```markdown
Co-Authored-By: <AI name> <noreply@example.com>
```

---

## GitHub CLI 사용

* **GH 사용 필수**: PR 생성 및 수정 시에는 GitHub CLI(`gh`)를 사용

---

## 작업 순서

### 0. GitHub CLI 권한 확인

```bash
command -v gh
gh auth status
gh repo view
```

설치되어 있지 않다면: `brew install gh`
인증이 필요하다면: `gh auth login`

### 1. Base 브랜치 자동 감지

**Git / Base branch 규칙:**
- 아래 스크립트로 현재 브랜치가 어떤 브랜치로부터 분기되었는지 찾는다.
- 스크립트로 계산된 결과 base 브랜치가 항상 develop인 것은 아니다.
- 감지된 base 브랜치를 신뢰하고 PR 작성 과정에도 그대로 사용한다.
- 작업자의 요청이 있기 전까지는 base 브랜치를 다시 추론하거나 develop/main/master 등의 브랜치로 교체하지 않는다.

```bash
CURRENT_BRANCH=$(git branch --show-current)
echo "Current branch: $CURRENT_BRANCH"

REPO_ROOT=$(git rev-parse --show-toplevel)
BASE_BRANCH=$(bash "$REPO_ROOT/.claude/scripts/find_base_branch.sh") || exit 1
echo "Detected base branch: $BASE_BRANCH"
```

### 2. 변경 사항 분석

base 브랜치와의 차이를 확인하여 PR 본문을 작성할 정보를 수집한다.

```bash
# 변경된 파일 목록 확인
git diff origin/${BASE_BRANCH}...HEAD --name-status

# 변경 통계 확인
git diff origin/${BASE_BRANCH}...HEAD --stat

# 커밋 히스토리 확인
git log origin/${BASE_BRANCH}..HEAD --oneline
```

### 3. PR 본문 작성

아래 템플릿을 기반으로 변경 사항을 분석하여 PR 본문을 작성한다.
**최종적으로 해당 PR에 포함된 실제 변경 사항**에 대한 내용만 작성한다.
없는 내용이나 코드를 추측해서 만들어내지 않는다.

### 4. PR 생성

```bash
# PR 본문을 임시 파일로 저장
cat > /tmp/pr_body.md <<'EOF'
(아래 템플릿 기반으로 작성한 PR 본문)
EOF

# 감지한 base 브랜치를 향해 PR 생성
gh pr create --base "$BASE_BRANCH" --title "PR 제목" --body-file /tmp/pr_body.md

# 임시 파일 정리
rm /tmp/pr_body.md
```

---

## PR 작성 원칙

* **한국어 작성**: 모든 PR 제목과 본문은 한국어로 작성
* **`--aiauthor` 사용 시 PR 본문 마지막에 AI 작성자 표기를 추가**
* **테스트 가이드 제외**: PR 본문에 테스트 가이드나 테스트 계획은 포함하지 않음

---

## PR 업데이트 시

* 기존 PR 본문을 적절하게 수정하여 최신 내용을 업데이트

---

## 완료 확인

* PR 생성 또는 업데이트 후 반드시 `git push`가 완료되었는지 확인

---

## PR 본문 템플릿

```markdown
## 관련 자료
- [슬랙 스레드]()
- [노션 업무 티켓]()
- [작업 계획](.claude/plan/...) // .claude/plan/...에 브랜치 명과 동일한 파일이 존재하면 첨부, 아니면 제외
- 이슈 : #

## 어떤 작업인가요?
- 리뷰어를 위한 PR 한 줄 설명 (PR 개요, 목적 등 '작업 배경' 설명)

## 어떻게 해결했나요?
**주요 작업 1**
- 작업 목적
- 작업 내용 요약

**주요 작업 2**
- 작업 목적
- 작업 내용 요약

...


## 중요한 변경 사항은 무엇인가요?
### 주요 작업 제목 1

**해당 작업의 핵심 변경사항 1**
- **변경 이유**: ...
- **수정 파일**: ...

**해당 작업의 핵심 변경사항 2**
- **변경 이유**: ...
- **수정 파일**: ...

### 주요 작업 제목 2

**해당 작업의 핵심 변경사항 1**
- **변경 이유**: ...
- **수정 파일**: ...

**해당 작업의 핵심 변경사항 2**
- **변경 이유**: ...
- **수정 파일**: ...

...


## 스크린샷 및 시연 영상 (optional)

### 코드 리뷰 RCA 룰
리뷰 코멘트의 중요도에 따라 접두에 R, C, A 중 하나를 붙인다.
- **R**(Request changes): 적극적으로 고려하거나 반드시 반영해 주세요.
- **C**(Comment): 가능한 반영해 주세요.
- **A**(Approve): 사소한 의견이므로 반영하지 않고 넘어가도 됩니다.
```
