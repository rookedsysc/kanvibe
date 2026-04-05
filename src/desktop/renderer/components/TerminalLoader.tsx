import Terminal from "@/desktop/renderer/components/Terminal";

interface TerminalLoaderProps {
  taskId: string;
}

export default function TerminalLoader({ taskId }: TerminalLoaderProps) {
  return (
    <div className="h-full">
      <Terminal taskId={taskId} />
    </div>
  );
}
