---
name: commit-skill
description: Conventional Commit Messages. 커밋 메시지 작성 및 PR 생성 지원. --only-commit 플래그로 커밋만 수행, --aiauthor 플래그로 AI 작성자 표기를 포함할 수 있으며, 플래그가 없으면 커밋 후 Push 및 PR 생성까지 진행.
---

## Flags

- `--only-commit`: 커밋만 수행 (Push/PR 생성 없음)
- `--aiauthor`: 커밋과 PR 본문에 AI 작성자 표기를 포함
- (기본): 커밋 → Push → PR 생성까지 **확인 없이 자동 진행** → [references/pr-guide.md](references/pr-guide.md) 참조

> **주의**: `--only-commit` 플래그가 없으면 사용자에게 PR 생성 여부를 묻지 않고 즉시 커밋 → Push → PR 생성을 순차 실행한다.

---

## AI 작성자 표기

기본 동작에서는 AI 작성자 표기를 추가하지 않는다.

`--aiauthor` 플래그가 있으면 커밋 메시지 footer와 PR 본문 마지막에 AI 작성자 표기를 포함한다.

커밋 시 HEREDOC 형식을 사용하고, `--aiauthor`가 있으면 아래처럼 footer를 추가한다:
```bash
git commit -m "$(cat <<'EOF'
feat: 커밋 메시지 내용

Co-Authored-By: <AI name> <noreply@example.com>
EOF
)"
```

---

## 커밋 워크플로우

### 1단계: Base 브랜치 감지

커밋 전 현재 브랜치의 base 브랜치를 감지하여 변경 범위를 파악한다.

```bash
CURRENT_BRANCH=$(git branch --show-current)
REPO_ROOT=$(git rev-parse --show-toplevel)
BASE_BRANCH=$(bash "$REPO_ROOT/.claude/scripts/find_base_branch.sh") || exit 1
echo "Current: $CURRENT_BRANCH / Base: $BASE_BRANCH"
```

### 2단계: 변경 사항 분석

base 브랜치 기준으로 변경 사항을 분석하여 커밋 단위를 결정한다.

```bash
# base 브랜치 대비 변경된 파일 목록
git diff origin/${BASE_BRANCH}...HEAD --name-status

# 아직 커밋되지 않은 변경 사항
git diff --name-status
git diff --cached --name-status
```

### 3단계: 커밋 단위 결정

- base 브랜치 대비 전체 변경 사항을 파악한 뒤, 논리적으로 관련된 변경끼리 그룹핑
- 각 커밋은 하나의 의미 있는 변경 단위로 구성
- 이미 커밋된 내용과 중복되지 않도록 `git log origin/${BASE_BRANCH}..HEAD --oneline`으로 기존 커밋 확인

### 4단계: 커밋 실행

```bash
git add <관련 파일들>
git commit -m "$(cat <<'EOF'
<type>(<optional scope>): <한국어 설명>
EOF
)"
```

---

## Commit Message Formats

### Default

<pre>
<b>&lt;type&gt;</b>(<b>&lt;optional scope&gt;</b>): <b>&lt;description&gt;</b>
<sub>empty separator line</sub>
<b>&lt;optional body&gt;</b>
<sub>empty separator line</sub>
<b>&lt;optional footer&gt;</b>
</pre>

### Merge Commit

<pre>
Merge branch '<b>&lt;branch name&gt;</b>'
</pre>

### Revert Commit

<pre>
Revert "<b>&lt;reverted commit subject line&gt;</b>"
</pre>

### Initial Commit

```
chore: init
```

### Types

* API or UI relevant changes
  * "feat" Commits, that add or remove a new feature to the API or UI
  * "fix" Commits, that fix an API or UI bug of a preceded "feat" commit
  * "modify" Commits, that change existing functionality or behavior
* "refactor" Commits, that rewrite/restructure your code, however do not change any API or UI behaviour
  * "perf" Commits are special "refactor" commits, that improve performance
* "style" Commits, that do not affect the meaning (white-space, formatting, missing semi-colons, etc)
* "test" Commits, that add missing tests or correcting existing tests
* "docs" Commits, that affect documentation only
* "build" Commits, that affect build components like build tool, ci pipeline, dependencies, project version, ...
* "ops" Commits, that affect operational components like infrastructure, deployment, backup, recovery, ...
* "chore" Miscellaneous commits e.g. modifying ".gitignore"

### Scopes

The "scope" provides additional contextual information.

* Is an **optional** part of the format
* Allowed Scopes depend on the specific project
* Don't use issue identifiers as scopes

### Breaking Changes Indicator

Breaking changes should be indicated by adding "!" before ":" in the subject line e.g. "feat(api)!: remove status endpoint"

* Is an **optional** part of the format
* Breaking changes **must** be described in the commit footer section

### Description

* It is a **mandatory** part of the format
* Use the imperative, present tense: "change" not "changed" nor "changes"
* Don't capitalize the first letter
* No dot (".") at the end

### Body

* Is an **optional** part of the format
* Use the imperative, present tense
* This is the place to mention issue identifiers and their relations

### Footer

* Is an **optional** part of the format
* **optionally** reference an issue by its id.
* **Breaking Changes** should start with the word "BREAKING CHANGE:" followed by space or two newlines.

### Examples

```
feat: 이메일 알림 기능 추가
```

```
feat(shopping cart): 장바구니에 즉시구매 버튼 추가
```

```
feat!: 티켓 목록 엔드포인트 제거

refers to JIRA-1337

BREAKING CHANGE: ticket endpoints no longer supports list all entities.
```

```
fix(api): 요청 body checksum 계산 오류 수정
```

```
perf: HyperLogLog 적용으로 고유 방문자 집계 메모리 사용량 감소
```

```
refactor: 피보나치 수열 계산을 재귀 방식으로 변경
```

---

## 커밋 작성 지침

### 중요 사항

* **모든 커밋 메시지는 한국어로 작성**
* **`--aiauthor` 사용 시 AI 작성자 표기를 커밋 footer와 PR 본문에 함께 반영**
* git diff를 통해 변경 사항을 충분히 분석하고, 논리적으로 커밋을 분리
* 각 커밋은 하나의 의미 있는 변경 단위로 구성

### 커밋 전 체크리스트

1. `git diff`로 모든 변경 사항 확인
2. **절대 커밋하지 말아야 할 파일 확인**
   * `.claude/`, `.gemini/`, `.opencode/`, hooks 등 AI 에이전트 관련 설정 파일은 커밋 금지
   * `CLAUDE.local.md`, `GEMINI.md`, `AGENTS.md` 등 AI 설정 문서도 커밋 금지
   * **예외**: `.claude/plan/` 폴더는 커밋 가능
   * 위 파일들이 staging area에 포함되어 있다면 반드시 제거
3. **Serena 관련 파일은 반드시 커밋**
   * `.serena/` 폴더 내 변경 사항이 있으면 누락 없이 커밋에 포함
   * `git diff --name-only`와 `git status`로 `.serena/` 변경 파일을 확인하고, 있으면 반드시 staging에 추가
4. 관련된 변경 사항끼리 그룹핑하여 커밋 단위 결정
5. **문서 변경사항 확인** → `docs:` 접두어 사용
6. 커밋 메시지는 변경의 "이유"와 "무엇"을 명확히 설명
7. `--aiauthor` 사용 여부에 맞게 AI 작성자 표기가 포함되었는지 최종 확인
