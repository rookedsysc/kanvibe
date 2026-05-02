"use client";

import { useEffect } from "react";

interface UseEscapeKeyOptions {
  enabled?: boolean;
}

export function useEscapeKey(
  onEscape: () => void,
  { enabled = true }: UseEscapeKeyOptions = {},
) {
  useEffect(() => {
    if (!enabled) {
      return;
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") {
        return;
      }

      event.preventDefault();
      onEscape();
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [enabled, onEscape]);
}
