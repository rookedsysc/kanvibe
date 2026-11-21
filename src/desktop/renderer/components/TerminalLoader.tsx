import { useTranslations } from "next-intl";
import InlineLoadError from "@/components/InlineLoadError";
import Terminal from "@/desktop/renderer/components/Terminal";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

interface TerminalLoaderProps {
  taskId: string;
  /**
   * terminal 세션의 탭 목록.
   * tmux와 zellij는 멀티플렉서가 화면을 하나로 그리므로 비워 두고 xterm 하나만 띄운다.
   */
  tabs?: TerminalTab[];
  /** 원격(SSH) 세션인지 여부. `Terminal`로 그대로 전달해 클립보드 이미지 붙여넣기 범위를 가른다 */
  isRemote?: boolean;
  isHidden?: boolean;
  taskTitle?: string;
  onHistory?: () => void;
  /** 탭 목록을 읽지 못한 이유. 비어 있는 목록이 실패 때문인지 가른다 */
  tabsError?: string | null;
  /** 첫 탭 목록 조회가 끝났는지. 끝나기 전의 빈 목록은 아직 기다리는 중이다 */
  isTabListLoaded?: boolean;
  onRetryTabs?: () => void;
  onCreateTab?: () => void;
}

/**
 * terminal 세션은 탭마다 xterm을 따로 두고 비활성 탭을 숨기기만 한다.
 * 언마운트하면 그 탭의 스크롤백이 사라져서, 돌아왔을 때 화면이 비어 보인다.
 */
export default function TerminalLoader({
  taskId,
  tabs,
  isRemote,
  isHidden = false,
  taskTitle,
  onHistory,
  tabsError = null,
  isTabListLoaded = false,
  onRetryTabs,
  onCreateTab,
}: TerminalLoaderProps) {
  const t = useTranslations("taskDetail");
  const tc = useTranslations("common");

  if (!tabs) {
    return (
      <div className="h-full">
        <Terminal taskId={taskId} isRemote={isRemote} isHidden={isHidden} taskTitle={taskTitle} onHistory={onHistory} />
      </div>
    );
  }

  /**
   * 탭 목록이 아직 안 왔을 때 탭 없는 터미널을 띄우면 안 된다.
   * terminal 세션의 PTY는 클라이언트가 떠나도 살아남아서, 버려진 PTY가 그대로 남는다.
   */
  if (tabs.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        {tabsError ? (
          <InlineLoadError
            className="w-full max-w-md"
            message={tc("loadFailed", { reason: tabsError })}
            onRetry={() => onRetryTabs?.()}
          />
        ) : !isTabListLoaded ? (
          <p role="status" className="flex items-center gap-2 text-xs text-terminal-text opacity-70">
            <span aria-hidden="true" className="h-3 w-3 rounded-full border-2 border-current border-t-transparent motion-safe:animate-spin" />
            {tc("loading")}
          </p>
        ) : (
          <button
            type="button"
            onClick={() => onCreateTab?.()}
            className="rounded-md border border-white/10 px-3 py-1.5 text-xs text-terminal-text transition-colors hover:bg-white/10"
          >
            {t("newTerminalTab")}
          </button>
        )}
      </div>
    );
  }

  return (
    <div className="h-full">
      {tabs.map((tab) => (
        <div key={tab.id} className={tab.isActive ? "h-full" : "hidden"}>
          <Terminal taskId={taskId} tabId={tab.id} isHidden={isHidden || !tab.isActive} isRemote={isRemote} taskTitle={taskTitle} onHistory={onHistory} />
        </div>
      ))}
    </div>
  );
}
