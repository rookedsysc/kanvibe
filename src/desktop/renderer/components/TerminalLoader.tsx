import Terminal from "@/desktop/renderer/components/Terminal";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

interface TerminalLoaderProps {
  taskId: string;
  /**
   * terminal 세션의 탭 목록.
   * tmux와 zellij는 멀티플렉서가 화면을 하나로 그리므로 비워 두고 xterm 하나만 띄운다.
   */
  tabs?: TerminalTab[];
}

/**
 * terminal 세션은 탭마다 xterm을 따로 두고 비활성 탭을 숨기기만 한다.
 * 언마운트하면 그 탭의 스크롤백이 사라져서, 돌아왔을 때 화면이 비어 보인다.
 */
export default function TerminalLoader({ taskId, tabs }: TerminalLoaderProps) {
  if (!tabs) {
    return (
      <div className="h-full">
        <Terminal taskId={taskId} />
      </div>
    );
  }

  /**
   * 탭 목록이 아직 안 왔을 때 탭 없는 터미널을 띄우면 안 된다.
   * terminal 세션의 PTY는 클라이언트가 떠나도 살아남아서, 버려진 PTY가 그대로 남는다.
   */
  if (tabs.length === 0) {
    return <div className="h-full" />;
  }

  return (
    <div className="h-full">
      {tabs.map((tab) => (
        <div key={tab.id} className={tab.isActive ? "h-full" : "hidden"}>
          <Terminal taskId={taskId} tabId={tab.id} isHidden={!tab.isActive} />
        </div>
      ))}
    </div>
  );
}
