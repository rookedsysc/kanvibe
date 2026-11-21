import { useEffect } from "react";
import { useTranslations } from "next-intl";
import type { BoardEventPayload } from "@/lib/boardNotifier";
import { claimHerdrServerHandoffPrompt, replaceHerdrServer } from "@/desktop/renderer/actions/sessionDependency";

/**
 * 원격 Herdr 서버가 이전 방식으로 떠 있다는 알림을 받으면 교체 여부를 묻는다.
 * 원격 도구 설치 확인과 같은 확인 창을 쓰며, 같은 서버는 앱이 실행되는 동안 한 번만 묻는다.
 * 교체 결과(실패 포함)는 main이 보드 알림으로 따로 알리므로 여기서는 요청만 보낸다.
 */
export default function HerdrServerHandoffPrompt() {
  const tCommon = useTranslations("common");

  useEffect(() => {
    return window.kanvibeDesktop!.onBoardEvent((event: BoardEventPayload) => {
      if (event.type !== "herdr-server-handoff-required") return;
      void (async () => {
        if (!(await claimHerdrServerHandoffPrompt(event.sshHost, event.herdrSession))) return;
        if (!window.confirm(tCommon("sessionDependency.herdrServerHandoffPrompt", { host: event.sshHost }))) return;
        await replaceHerdrServer(event.sshHost, event.herdrSession);
      })().catch((error: unknown) => console.error("Herdr 서버 교체 요청 실패:", error));
    });
  }, [tCommon]);

  return null;
}
