import { useEffect, useState } from "react";
import Terminal from "@/desktop/renderer/components/Terminal";

interface TerminalLoaderProps {
  taskId: string;
}

export default function TerminalLoader({ taskId }: TerminalLoaderProps) {
  const [shouldMountTerminal, setShouldMountTerminal] = useState(false);

  useEffect(() => {
    const scheduleMount = window.requestIdleCallback
      ? window.requestIdleCallback(() => setShouldMountTerminal(true))
      : window.setTimeout(() => setShouldMountTerminal(true), 0);

    return () => {
      if (typeof scheduleMount === "number") {
        window.clearTimeout(scheduleMount);
        return;
      }

      window.cancelIdleCallback?.(scheduleMount);
    };
  }, []);

  return (
    <div className="h-full">
      {shouldMountTerminal ? <Terminal taskId={taskId} /> : <div className="h-full bg-terminal-bg" />}
    </div>
  );
}
