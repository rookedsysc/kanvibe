---
name: search-and-anki-make-skill
description: "Generate comprehensive Anki cards for technical concepts and interview preparation. Use when the user requests: (1) Creating study materials for technical concepts or frameworks, (2) Evaluating interview answers (technical or experience-based), (3) Generating flashcards or review materials, (4) Researching technical topics for learning purposes. Supports both pure technical Q&A (using CLEAR framework) and project experience Q&A (using 선택의 흐름 framework). Keywords that trigger this skill include \"anki\", \"면접\", \"interview\", \"카드\", \"card\", \"평가\", \"evaluate\", question-answer pairs about technical concepts, or requests to research/document technical topics."
---

# Anki Interview Card Generator

Generate high-quality Anki cards for technical interview preparation with a two-part structure:
1. **💬 면접 답변** - Natural speaking format (2-3 minutes) for interview practice
2. **📋 상세 분석** - Structured analysis using proven frameworks (CLEAR or 선택의 흐름)

Each card includes thorough research, source citations, code examples, and trade-off analysis.

## Workflow

Follow these steps for every request:

### Step 1: Classify the Input Type

Determine which framework to use based on the user's input:

1. **Technical Interview Q&A**: Pure technical/concept questions
   - Examples: "REST API란?", "OOP 4대 원칙", "트랜잭션 ACID", "Java의 특징"
   - Framework: CLEAR (Concept, Logic, Example, Application, Relation)
   - Output: 💬 면접 답변 + 📋 상세 분석 (CLEAR)
   - Reference: [technical-interview.md](references/technical-interview.md)

2. **Experience Interview Q&A**: Project experience, problem-solving, system design
   - Examples: "성능 개선 경험", "어려웠던 기술 문제", "팀 협업 사례"
   - Framework: 선택의 흐름 (Problem, Options, Decision, Action, Result)
   - Output: 💬 면접 답변 + 📋 상세 분석 (선택의 흐름)
   - Reference: [experience-interview.md](references/experience-interview.md)

3. **Simple Research**: General technical topic investigation without existing answer
   - Examples: "React hooks 조사해줘", "Kubernetes 개념 정리"
   - Framework: Tech Search Guide + CLEAR Format
   - Output: 💬 면접 답변 + 📋 상세 분석 (CLEAR)
   - Reference: [tech-search-guide.md](references/tech-search-guide.md)

### Step 2: Research the Topic (if needed)

For research tasks or when creating model answers:

1. **Search Strategy**:
   - Use WebSearch for latest information or lesser-known topics
   - Use Context7 (--c7) for official documentation, library usage, framework patterns
   - Always cite sources with links

2. **Content Requirements**:
   - Start with conclusion (answer-first approach)
   - Provide concrete examples with working code
   - Include trade-offs and alternatives
   - Use natural, blog-quality writing style
   - Maintain consistent terminology

See [tech-search-guide.md](references/tech-search-guide.md) for detailed search guidelines.

### Step 2.5: Present Research Results (Optional)

**When to use**: Only if the user explicitly requests to see research results separately before card generation.

Otherwise, proceed directly to Step 4 (Generate Anki Card) which will include all research findings in a structured format.

### Step 3: Evaluate Existing Answer (if provided)

If the user provided their own answer to evaluate:

**For Technical Questions** (CLEAR framework):
- Evaluate across 5 dimensions (20 points each):
  1. Concept: Accurate definition in one sentence?
  2. Logic: Explains why/how it works?
  3. Example: Concrete code or analogy?
  4. Application: Real-world use cases?
  5. Relation: Related concepts, trade-offs, alternatives?

**For Experience Questions** (선택의 흐름):
- Evaluate across 5 stages (20 points each):
  1. Problem: What problem? Why? How measured?
  2. Options: What alternatives considered? Pros/cons?
  3. Decision: Why this choice? Team consensus?
  4. Action: How implemented? What problems faced?
  5. Result: Measurable outcomes? Learnings?

Provide detailed feedback on strengths, weaknesses, and improvement areas for each dimension.

### Step 4: Generate Anki Card

Create the final Anki card in MDX format following this structure:

```markdown
## [Question Title]

<details>
<summary>꼬리 질문</summary>
<div>

- [Follow-up question testing depth]
- [Follow-up question testing trade-offs]
- [Follow-up question testing practical application]

</div>
</details>

<details>
<summary>답변 보기</summary>
<div>

💬 **면접 답변 (2-3분 분량)**

[자연스러운 면접 답변을 여러 단락으로 작성. 서론-본론-결론 구조로 흐름있게 작성]

[첫 단락: 개념 정의와 핵심 특징 소개]

[중간 단락들: 주요 특징을 "첫째, 둘째, 셋째" 등으로 나열하며 구체적 설명]

[마지막 단락: 장단점 또는 사용 사례, 결론]

---

📋 **상세 분석 (암기/복습용)**

**핵심 포인트 💡**
- [Key point 1 - 핵심 개념 요약]
- [Key point 2 - 주요 특징 요약]
- [Key point 3 - 장점 요약]
- [Key point 4 - 단점 요약]

**Concept - 개념 정의**

[개념에 대한 정확한 정의와 설명]

- 출처: [Source Name](link)
- > 위 출처에서 "실제 원문 인용" 이라고 명시되어 있음.

**Logic - 원리/동작 방식**

[왜 그렇게 동작하는지, 내부 원리 설명]

[필요시 순서가 있는 동작은 번호로 나열]

- 출처: [Source Name](link)
- > 위 출처에서 "실제 원문 인용" 이라고 명시되어 있음.

**Example - 예시/비유**

[구체적인 예시 설명]

```language
// 동작하는 실제 코드 예시
```

[추가 예시가 있다면 계속 작성]

```language
// 비교 코드 (Before/After, Good/Bad 등)
```

**Application - 활용/적용**

[언제, 어디서 사용하는지]

[사용하는 경우:]
- [Use case 1]
- [Use case 2]

[사용하지 않는 경우:]
- [Anti-pattern 1] → [Alternative]
- [Anti-pattern 2] → [Alternative]

**Relation - 관련 개념/심화**

[관련 개념:]
- **[Related concept 1]**: [간단한 설명]
- **[Related concept 2]**: [간단한 설명]

[필요시 비교 테이블:]

| 구분 | A | B | C |
|------|---|---|---|
| 특징1 | ... | ... | ... |
| 특징2 | ... | ... | ... |

[트레이드오프:]

- 장점:
  - [장점 1 with 구체적 설명]
  - [장점 2 with 구체적 설명]
- 단점:
  - [단점 1 with 구체적 설명]
  - [단점 2 with 구체적 설명]

</div>
</details>

<details>
<summary>참고 자료</summary>
<div>

### 조사 출처
- [Source 1 Name](link) - 주요 기여 내용 요약
- [Source 2 Name](link) - 주요 기여 내용 요약
- [Source 3 Name](link) - 주요 기여 내용 요약

### 추가 학습 자료
- [Related Documentation](link) - 연관 개념 또는 심화 학습
- [Tutorial or Guide](link) - 실습 자료

</div>
</details>
```

**Critical Requirements**:

1. **면접 답변 섹션**:
   - 2-3분 분량의 자연스러운 말투 (구어체)
   - "~입니다", "~했습니다" 체 사용
   - 여러 단락으로 구성하여 흐름있게 작성
   - 섹션 헤더 없이 자연스럽게 연결
   - "첫째, 둘째, 셋째" 등으로 항목 나열
   - 서론-본론-결론 구조

2. **상세 분석 섹션**:
   - CLEAR 프레임워크 준수 (Concept → Logic → Example → Application → Relation)
   - 각 섹션마다 출처 명시
   - 실제 동작하는 코드 예시 포함
   - 비교 테이블 적극 활용
   - 트레이드오프 명시 (장점/단점)

3. **출처 표기**:
   - `[Source Name](link)` 형식
   - 반드시 원문 인용구 포함: `> 위 출처에서 "quote" 이라고 명시되어 있음.`

4. **포맷팅**:
   - `<div>` 다음 한 줄 띄우기
   - **절대 h 태그 사용 금지** (###, #### 등)
   - 구조화는 불릿 포인트와 **볼드** 텍스트로
   - NO placeholders - 모든 내용을 실제로 작성

See [anki-format.md](references/anki-format.md) for complete formatting guidelines.

### Step 5: Save to File

Save the generated card as an MDX file:

1. **Filename generation**:
   - If user specifies filename (`@파일명` or `파일명에 {저장/넣어/작성}줘`): use specified name
   - If evaluation context: `{original-filename}-anki.md`
   - Otherwise: auto-generate from topic (e.g., `react-useEffect정리함수.mdx`)

2. **File writing**:
   - Use Bash heredoc for Korean/emoji content (per CLAUDE.local.md)
   - Check if file exists first with Read tool
   - Save in current working directory unless specified

## Quality Checklist

Before delivering, verify:

### Interview Answer Section (💬 면접 답변)
- [ ] 2-3분 분량으로 작성됨
- [ ] 자연스러운 구어체 ("~입니다", "~했습니다")
- [ ] 여러 단락으로 흐름있게 구성 (서론-본론-결론)
- [ ] 섹션 헤더 없이 자연스럽게 연결
- [ ] "첫째, 둘째, 셋째" 등으로 항목 구분
- [ ] 실제 면접장에서 말할 수 있는 톤

### Detailed Analysis Section (📋 상세 분석)
- [ ] 핵심 포인트 💡 섹션 포함 (3-4개 불릿)
- [ ] CLEAR 프레임워크 완전히 준수:
  - [ ] Concept - 개념 정의
  - [ ] Logic - 원리/동작 방식
  - [ ] Example - 예시/비유 (코드 포함)
  - [ ] Application - 활용/적용
  - [ ] Relation - 관련 개념/심화
- [ ] 각 섹션마다 출처 + 원문 인용 포함
- [ ] 비교 테이블 적절히 사용
- [ ] 트레이드오프 명시 (장점/단점)

### Technical Accuracy
- [ ] Technical content is factually correct
- [ ] Information is current/up-to-date
- [ ] Aligns with official documentation
- [ ] Every claim has a source citation

### Format Compliance
- [ ] Source links in `[Name](url)` format
- [ ] Original quotes in `> 위 출처에서 "quote" 이라고 명시되어 있음.` format
- [ ] Working code examples (not pseudocode)
- [ ] Blank lines after `<div>` tags
- [ ] **NO h tags** (###, ####) - only bold text and bullets
- [ ] "참고 자료" section included with sources
- [ ] NO placeholders - all content fully written

## Important Notes

- **Two-part structure**: Every card MUST have both:
  1. 💬 면접 답변: Natural speaking format (2-3 minutes)
  2. 📋 상세 분석: CLEAR framework with sources

- **Interview answer style**:
  - Write as if speaking to an interviewer
  - Natural flow with multiple paragraphs
  - "첫째, 둘째, 셋째" for listing points
  - NO section headers in this part
  - Focus on clarity and deliverability

- **Detailed analysis requirements**:
  - Must include all 5 CLEAR components
  - Each component needs sources with quotes
  - Code examples must be executable
  - Comparison tables for related concepts
  - Trade-offs clearly stated

- **Answer completeness**: Never use placeholders like `[content]` - always write actual, usable content
- **Source attribution**: Every technical claim needs a source link + original quote in format: `> 위 출처에서 "quote" 이라고 명시되어 있음.`
- **Depth over breadth**: Better to cover essential points thoroughly than skim many topics
- **No h tags**: Use **bold text** and bullet depth for structure, never ### or ####
