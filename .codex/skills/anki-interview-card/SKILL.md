---
name: anki-interview-card
description: "Generate comprehensive Anki cards for technical concepts and interview preparation. Use when the user requests: (1) Creating study materials for technical concepts or frameworks, (2) Evaluating interview answers (technical or experience-based), (3) Generating flashcards or review materials, (4) Researching technical topics for learning purposes. Supports both pure technical Q&A (using CLEAR framework) and project experience Q&A (using 선택의 흐름 framework). Keywords that trigger this skill include \"anki\", \"면접\", \"interview\", \"카드\", \"card\", \"평가\", \"evaluate\", question-answer pairs about technical concepts, or requests to research/document technical topics."
---

# Anki Interview Card Generator

Generate high-quality Anki cards for technical interview preparation by researching topics and formatting answers according to proven evaluation frameworks.

## Workflow

Follow these steps for every request:

### Step 1: Classify the Input Type

Determine which framework to use based on the user's input:

1. **Technical Interview Q&A**: Pure technical/concept questions
   - Examples: "REST API란?", "OOP 4대 원칙", "트랜잭션 ACID"
   - Framework: CLEAR (Concept, Logic, Example, Application, Relation)
   - Reference: [technical-interview.md](references/technical-interview.md)

2. **Experience Interview Q&A**: Project experience, problem-solving, system design
   - Examples: "성능 개선 경험", "어려웠던 기술 문제", "팀 협업 사례"
   - Framework: 선택의 흐름 (Problem, Options, Decision, Action, Result)
   - Reference: [experience-interview.md](references/experience-interview.md)

3. **Simple Research**: General technical topic investigation without existing answer
   - Examples: "React hooks 조사해줘", "Kubernetes 개념 정리"
   - Framework: Tech Search Guide + Anki Format
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

**[Main answer in natural interview tone]**

- 핵심 포인트 💡:
  - [Key point 1 with specific details]
  - [Key point 2 with specific details]
  - [Key point 3 with specific details]

[Detailed sections with sources]
- [Content] [Source Name](link)
> 위 출처에서 "actual quote" 이라고 명시되어 있음.

[Code examples if applicable]
```language
// Concrete, working code
```

[Practical usage with ✅ correct / ❌ common mistakes]

---

**꼬리 질문 1: [Repeat question]**

💬 면접 답변 (1-2분 분량)

[Complete interview-ready answer in natural speaking tone. Flow: concept → principle → example → application]

- 출처: [Source](link)
- > 위 출처에서 "quote" 이라고 명시되어 있음.

</div>
</details>
```

**Critical Requirements**:
- NO placeholders - write actual content
- Include sources with format: `[Source Name](link)` followed by `> 위 출처에서 "quote" 이라고 명시되어 있음.`
- Use natural "~입니다", "~했습니다" interview speaking style
- Follow structure: concept → principle → example → application → related concepts
- Include working code examples
- Add blank line between `<div>` and markdown content
- Use list depth (li tags) instead of h tags for organization

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

### Technical Accuracy
- [ ] Technical content is factually correct
- [ ] Information is current/up-to-date
- [ ] Aligns with official documentation
- [ ] Sources are cited for all claims

### Answer Quality
- [ ] Addresses the question directly
- [ ] No unnecessary tangents
- [ ] Concise and focused
- [ ] Natural reading flow
- [ ] No grammatical errors
- [ ] Consistent terminology

### Format Compliance
- [ ] Source links in `[Name](url)` format
- [ ] Original quotes provided
- [ ] Working code examples (if included)
- [ ] Proper markdown structure
- [ ] Blank lines after `<div>` tags
- [ ] No h tags (use li tag depth)

## Important Notes

- **Answer completeness**: Never use placeholders like `[content]` - always write actual, usable content
- **Source attribution**: Every technical claim needs a source link + original quote
- **Interview readiness**: Answers should be speakable in an interview setting
- **Depth over breadth**: Better to cover essential points thoroughly than skim many topics
- **Measurement focus**: For experience questions, always include specific metrics and outcomes
