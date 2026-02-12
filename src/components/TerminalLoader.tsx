"use client";

import dynamic from "next/dynamic";

const Terminal = dynamic(() => import("@/components/Terminal"), { ssr: false });

interface TerminalLoaderProps {
  taskId: string;
}

/** xterm.js는 SSR 불가이므로 Client Component에서 dynamic import한다 */
export default function TerminalLoader({ taskId }: TerminalLoaderProps) {
  return (
    <div className="h-full">
      <Terminal taskId={taskId} />
    </div>
  );
}
