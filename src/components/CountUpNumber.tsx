"use client";

import { useEffect, useRef, useState } from "react";

/** 한 번 굴러가는 데 걸리는 시간. 폴링 주기보다 훨씬 짧아 다음 값이 오기 전에 항상 멈춘다 */
const COUNT_UP_DURATION_MS = 420;

function prefersReducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

interface CountUpNumberProps {
  value: number;
  className?: string;
  testId?: string;
}

/**
 * 숫자를 이전 값에서 새 값으로 굴린다.
 *
 * 스스로 갱신되는 화면에서는 어느 자리가 방금 바뀌었는지 눈이 놓치기 쉬워, 움직임으로 그 자리를 가리킨다.
 * 움직임을 줄이도록 설정한 사용자에게는 굴리지 않고 새 값으로 바로 건너뛴다.
 */
export function CountUpNumber({ value, className, testId }: CountUpNumberProps) {
  const [displayedValue, setDisplayedValue] = useState(value);
  const [isCounting, setIsCounting] = useState(false);
  const displayedValueRef = useRef(value);
  displayedValueRef.current = displayedValue;

  useEffect(() => {
    const startValue = displayedValueRef.current;
    if (startValue === value) {
      return;
    }

    if (prefersReducedMotion() || typeof window.requestAnimationFrame !== "function") {
      setDisplayedValue(value);
      return;
    }

    const startedAt = performance.now();
    setIsCounting(true);

    let frameId = window.requestAnimationFrame(function advanceToNextFrame(now: number) {
      const progress = Math.min((now - startedAt) / COUNT_UP_DURATION_MS, 1);
      const easedProgress = 1 - (1 - progress) ** 3;
      setDisplayedValue(Math.round(startValue + (value - startValue) * easedProgress));

      if (progress < 1) {
        frameId = window.requestAnimationFrame(advanceToNextFrame);
        return;
      }

      setIsCounting(false);
    });

    return () => {
      window.cancelAnimationFrame(frameId);
      setIsCounting(false);
    };
  }, [value]);

  return (
    <span
      className={`kv-count-up tabular-nums${className ? ` ${className}` : ""}`}
      data-counting={isCounting || undefined}
      data-testid={testId}
    >
      {displayedValue}
    </span>
  );
}
