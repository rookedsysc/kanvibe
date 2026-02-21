/** 매칭된 문자를 하이라이트하여 렌더링한다 */
export default function HighlightedText({ text, matchedIndices }: { text: string; matchedIndices: number[] }) {
  const matchSet = new Set(matchedIndices);
  const segments: { text: string; highlighted: boolean }[] = [];
  let current = "";
  let currentHighlighted = false;

  for (let i = 0; i < text.length; i++) {
    const isMatch = matchSet.has(i);
    if (i === 0) {
      currentHighlighted = isMatch;
      current = text[i];
    } else if (isMatch === currentHighlighted) {
      current += text[i];
    } else {
      segments.push({ text: current, highlighted: currentHighlighted });
      current = text[i];
      currentHighlighted = isMatch;
    }
  }
  if (current) segments.push({ text: current, highlighted: currentHighlighted });

  return (
    <span className="font-mono text-xs">
      {segments.map((seg, i) =>
        seg.highlighted ? (
          <span key={i} className="text-brand-primary font-bold">{seg.text}</span>
        ) : (
          <span key={i}>{seg.text}</span>
        )
      )}
    </span>
  );
}
